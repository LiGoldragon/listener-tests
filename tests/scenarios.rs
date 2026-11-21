use listener_library::*;
use listener_memory::{
    ExecutingMemory, InspectingMemory, MemoryDefinition, OpeningMemory, operations::*, state::Audio,
};
use meta_signal_listener::Set;
use tempfile::TempDir;

// These constructors are test fixtures, not production entry points or clocks.
fn configuration(sequence: Sequence) -> Configuration {
    Configuration {
        backend_vector: vec![Backend {
            backend_name: BackendName("speech".into()),
            protocol: Protocol::OpenAiTranscription,
            service_address: ServiceAddress("https://fixture.invalid".into()),
            model_name: ModelName("model".into()),
            secret_entry: SecretEntry("fixture/key".into()),
            language: Language("en".into()),
            instruction: Instruction("transcribe".into()),
            timeout: Timeout(Seconds(30)),
        }],
        order: Order(vec![BackendName("speech".into())]),
        delivery: Delivery::Kept,
        sequence,
        audio_kept: AudioKept(Retention::Forever),
        text_kept: TextKept(Retention::Forever),
        term_vector: Vec::new(),
        kilobits: Kilobits(24),
        source: Source::Default,
        history_length: HistoryLength(20),
        paste_program: PasteProgram(Program("/fixture/paste".into())),
        copy_program: CopyProgram(Program("/fixture/copy".into())),
        secret_program: SecretProgram(Program("/fixture/secret".into())),
        audio_directory: AudioDirectory(Directory("/fixture/audio-before".into())),
        budget: Budget(translator::DatomReadingBudget {
            reading_steps: translator::ReadingSteps(1024),
            reading_depth: translator::ReadingDepth(64),
        }),
        recording_settings: RecordingSettings {
            pulse_server: PulseServer::Default,
            pulse_application_name: PulseApplicationName("fixture".into()),
            sample_rate: SampleRate(48000),
            channels: Channels::Mono,
            capture_fragment_bytes: CaptureFragmentBytes(3840),
            capture_buffer_bytes: CaptureBufferBytes(15360),
            pending_pcm_frames: PendingPcmFrames(9600),
            opus_frame_duration: OpusFrameDuration::TwentyMilliseconds,
            opus_application: OpusApplication::Voice,
            bitrate_control: BitrateControl::Variable,
            encoding_complexity: EncodingComplexity(5),
            audio_signal: AudioSignal::Voice,
            audio_bandwidth: AudioBandwidth::Fullband,
            discontinuous_transmission: DiscontinuousTransmission::Disabled,
            forward_error_correction: ForwardErrorCorrection::Disabled,
            expected_packet_loss: ExpectedPacketLoss(0),
            page_packets: PagePackets(1),
            audio_durability: AudioDurability::SyncEachPage,
            gap_handling: GapHandling::Interrupt,
        },
    }
}
fn definition(directory: &TempDir) -> MemoryDefinition {
    MemoryDefinition {
        path: directory.path().join("new-memory.redb"),
        table_name: "listener-state",
        record_key: "state".into(),
    }
}
fn request(input: Input, moment: i64, id: Option<&str>) -> Execution {
    Execution {
        input,
        moment: Moment(moment),
        job_id_option: id.map(|id| JobId(id.into())),
    }
}
fn execute(
    memory: &listener_memory::Memory,
    input: Input,
    moment: i64,
    id: Option<&str>,
) -> Committed {
    memory.execute(request(input, moment, id)).unwrap()
}
fn start(memory: &listener_memory::Memory, id: &str, moment: i64) -> Fresh_Data {
    let result = execute(
        memory,
        Input::Turn(Options {
            delivery_option: None,
            backend_name_vector_option: None,
        }),
        moment,
        Some(id),
    );
    assert_eq!(
        result.changes.job_state_vector,
        vec![JobState {
            job_id: JobId(id.into()),
            state: State::Recording
        }]
    );
    let Output::Turning(Turning::Fresh(fresh)) = result.output else {
        panic!("fresh recording expected")
    };
    assert_eq!(fresh.audio_location.job_id, JobId(id.into()));
    fresh
}
fn stop(memory: &listener_memory::Memory, id: &str) {
    assert_eq!(
        execute(
            memory,
            Input::Turn(Options {
                delivery_option: None,
                backend_name_vector_option: None
            }),
            0,
            None
        )
        .output,
        Output::Turning(Turning::Running(JobId(id.into())))
    );
}
fn heard(memory: &listener_memory::Memory, id: &str, text: &str) -> Next {
    let result = execute(
        memory,
        Input::Record(Record_Data {
            job_id: JobId(id.into()),
            hearing: Hearing::Heard(Heard_Data {
                transcript: Transcript {
                    backend_name: BackendName("speech".into()),
                    provider_id_option: None,
                    text: Text(text.into()),
                },
                try_vector: Vec::new(),
            }),
        }),
        0,
        None,
    );
    let Output::Next(next) = result.output else {
        panic!("next expected")
    };
    next
}
fn finish(memory: &listener_memory::Memory, id: &str) -> Next {
    let Output::Next(next) = execute(
        memory,
        Input::Finish(Finish_Data {
            job_id: JobId(id.into()),
            handing: Handing::Kept,
        }),
        0,
        None,
    )
    .output
    else {
        panic!("next expected")
    };
    next
}
fn ready_id(next: Next) -> String {
    let Next::Deliverable(ready) = next else {
        panic!("deliverable expected")
    };
    ready.job_id.0
}

#[test]
fn fresh_memory_and_remember_never_touch_old_stores_and_survive_reopen() {
    let directory = TempDir::new().unwrap();
    let legacy = directory.path().join("history.jsonl");
    let audio = directory.path().join("old-audio.log");
    std::fs::write(&legacy, b"old-text\n").unwrap();
    std::fs::write(&audio, b"old-audio\0").unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let committed = execute(
        &memory,
        Input::Import(Import_Data {
            moment: Moment(7),
            text: Text("remembered".into()),
        }),
        100,
        Some("old"),
    );
    assert_eq!(committed.output, Output::Imported(JobId("old".into())));
    let state = memory.inspect().unwrap();
    let job = &state.job_vector[0];
    assert_eq!(job.state, State::Delivered);
    assert_eq!(job.audio, Audio::Erased);
    assert!(job.transcript_vector.is_empty());
    assert!(job.backend_name_vector.is_empty());
    assert_eq!(job.imported_text.0, Some(Text("remembered".into())));
    assert_eq!(job.moment, Moment(7));
    drop(memory);
    let reopened = definition(&directory)
        .open(configuration(Sequence::AsReady))
        .unwrap();
    assert_eq!(reopened.inspect().unwrap(), state);
    assert!(matches!(
        execute(&reopened, Input::Requeue(JobId("old".into())), 200, None).output,
        Output::Requeueing(Requeueing::NoAudio(_))
    ));
    assert_eq!(std::fs::read(legacy).unwrap(), b"old-text\n");
    assert_eq!(std::fs::read(audio).unwrap(), b"old-audio\0");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
}

#[test]
fn both_sequence_policies_handle_out_of_order_completions_with_one_delivery_claim() {
    for sequence in [Sequence::AsRecorded, Sequence::AsReady] {
        let directory = TempDir::new().unwrap();
        let memory = definition(&directory)
            .open(configuration(sequence.clone()))
            .unwrap();
        start(&memory, "first", 10);
        stop(&memory, "first");
        start(&memory, "second", 11);
        stop(&memory, "second");
        let second = heard(&memory, "second", "two");
        match sequence {
            Sequence::AsRecorded => {
                assert_eq!(second, Next::Nothing);
                assert_eq!(ready_id(heard(&memory, "first", "one")), "first");
                assert_eq!(ready_id(finish(&memory, "first")), "second");
                assert_eq!(finish(&memory, "second"), Next::Nothing);
            }
            Sequence::AsReady => {
                assert_eq!(ready_id(second), "second");
                assert_eq!(heard(&memory, "first", "one"), Next::Nothing);
                assert_eq!(ready_id(finish(&memory, "second")), "first");
                assert_eq!(finish(&memory, "first"), Next::Nothing);
            }
        }
        assert!(
            memory
                .inspect()
                .unwrap()
                .job_vector
                .iter()
                .all(|job| job.state == State::Delivered)
        );
        assert!(
            memory
                .execute(request(
                    Input::Finish(Finish_Data {
                        job_id: JobId("first".into()),
                        handing: Handing::Pasted
                    }),
                    20,
                    None
                ))
                .is_err()
        );
    }
}

#[test]
fn exhausted_job_leaves_the_delivery_order() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    start(&memory, "first", 1);
    stop(&memory, "first");
    start(&memory, "second", 2);
    stop(&memory, "second");
    assert_eq!(heard(&memory, "second", "two"), Next::Nothing);
    let committed = execute(
        &memory,
        Input::Record(Record_Data {
            job_id: JobId("first".into()),
            hearing: Hearing::Exhausted(Vec::new()),
        }),
        3,
        None,
    );
    let Output::Next(next) = committed.output else {
        panic!()
    };
    assert_eq!(ready_id(next), "second");
    assert_eq!(memory.inspect().unwrap().job_vector[0].state, State::Failed);
}

#[test]
fn configuration_updates_are_atomic_and_jobs_snapshot_locations_and_recording_settings() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let first = start(&memory, "first", 1);
    stop(&memory, "first");
    let before = memory.inspect().unwrap();
    let changed = execute(
        &memory,
        Input::Apply(Set::AudioDirectory(Directory(
            "/fixture/audio-after".into(),
        ))),
        2,
        None,
    );
    assert!(matches!(
        changed.changes.set_option,
        Some(Set::AudioDirectory(_))
    ));
    let mut settings = before.configuration.recording_settings.clone();
    settings.encoding_complexity = EncodingComplexity(7);
    execute(
        &memory,
        Input::Apply(Set::RecordingSettings(settings.clone())),
        2,
        None,
    );
    let second = start(&memory, "second", 2);
    assert_ne!(
        first.audio_location.directory,
        second.audio_location.directory
    );
    assert_ne!(first.recording_settings, second.recording_settings);
    assert_eq!(second.recording_settings, settings);
    let state = memory.inspect().unwrap();
    assert_eq!(
        state.job_vector[0].recording_settings,
        first.recording_settings
    );
    let refused = execute(
        &memory,
        Input::Apply(Set::Order(vec![BackendName("missing".into())])),
        3,
        None,
    );
    assert!(matches!(
        refused.output,
        Output::Applying(Applying::UnknownBackend(_))
    ));
    assert!(refused.changes.set_option.is_none());
    assert_eq!(memory.inspect().unwrap(), state);
    assert!(
        memory
            .execute(request(
                Input::Apply(Set::HistoryLength(HistoryLength(-1))),
                3,
                None
            ))
            .is_err()
    );
    assert_eq!(memory.inspect().unwrap(), state);
    drop(memory);
    let reopened = definition(&directory)
        .open(configuration(Sequence::AsReady))
        .unwrap();
    assert_eq!(reopened.inspect().unwrap(), state);
}

#[test]
fn text_and_audio_expire_independently_and_only_matching_successful_deletions_are_forgotten() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let first = start(&memory, "first", 0);
    stop(&memory, "first");
    assert_eq!(ready_id(heard(&memory, "first", "one")), "first");
    finish(&memory, "first");
    execute(
        &memory,
        Input::Apply(Set::AudioDirectory(Directory("/fixture/changed".into()))),
        1,
        None,
    );
    let second = start(&memory, "second", 0);
    stop(&memory, "second");
    assert_eq!(ready_id(heard(&memory, "second", "two")), "second");
    finish(&memory, "second");
    start(&memory, "active", 0);
    execute(
        &memory,
        Input::Apply(Set::TextKept(Retention::Days(Days(1)))),
        0,
        None,
    );
    assert_eq!(
        execute(&memory, Input::Prune, 86401, None).output,
        Output::Pruned(Vec::new())
    );
    assert!(matches!(
        execute(&memory, Input::Find(JobId("first".into())), 86401, None).output,
        Output::Finding(Finding::NothingKept)
    ));
    assert!(
        memory
            .inspect()
            .unwrap()
            .job_vector
            .iter()
            .all(|job| matches!(job.audio, Audio::Kept(_)))
    );
    execute(
        &memory,
        Input::Apply(Set::AudioKept(Retention::Days(Days(1)))),
        0,
        None,
    );
    let locations = vec![first.audio_location.clone(), second.audio_location.clone()];
    assert_eq!(
        execute(&memory, Input::Prune, 86401, None).output,
        Output::Pruned(locations.clone())
    );
    let wrong = AudioLocation {
        directory: Directory("/fixture/wrong".into()),
        job_id: first.audio_location.job_id.clone(),
    };
    execute(&memory, Input::Forget(vec![wrong]), 86401, None);
    assert_eq!(
        execute(&memory, Input::Prune, 86401, None).output,
        Output::Pruned(locations)
    );
    let retired = execute(
        &memory,
        Input::Forget(vec![first.audio_location]),
        86401,
        None,
    );
    assert_eq!(retired.changes.retired.0, vec![JobId("first".into())]);
    assert_eq!(
        execute(&memory, Input::Prune, 86401, None).output,
        Output::Pruned(vec![second.audio_location])
    );
    let state = memory.inspect().unwrap();
    assert_eq!(state.job_vector.len(), 2);
    assert!(
        state
            .job_vector
            .iter()
            .any(|job| job.job_id.0 == "active" && job.state == State::Recording)
    );
}

#[test]
fn audio_expiry_preserves_text_and_recall_until_text_expiry() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let fresh = start(&memory, "job", 0);
    stop(&memory, "job");
    heard(&memory, "job", "text");
    finish(&memory, "job");
    execute(
        &memory,
        Input::Apply(Set::AudioKept(Retention::Days(Days(0)))),
        0,
        None,
    );
    execute(&memory, Input::Forget(vec![fresh.audio_location]), 10, None);
    assert!(matches!(
        execute(&memory, Input::Find(JobId("job".into())), 10, None).output,
        Output::Finding(Finding::Ready(_))
    ));
    assert!(matches!(
        execute(&memory, Input::Requeue(JobId("job".into())), 10, None).output,
        Output::Requeueing(Requeueing::NoAudio(_))
    ));
    execute(
        &memory,
        Input::Apply(Set::TextKept(Retention::Days(Days(0)))),
        10,
        None,
    );
    let result = execute(&memory, Input::Prune, 10, None);
    assert_eq!(result.changes.retired.0, vec![JobId("job".into())]);
    assert!(memory.inspect().unwrap().job_vector.is_empty());
}

#[test]
fn one_recording_stale_interrupt_and_refused_operations_leave_consistent_state() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    start(&memory, "first", 0);
    assert_eq!(
        execute(&memory, Input::Interrupt(JobId("stale".into())), 1, None).output,
        Output::Interrupting(Interrupting::NothingRecording)
    );
    assert_eq!(
        memory.inspect().unwrap().job_vector[0].state,
        State::Recording
    );
    assert_eq!(
        execute(&memory, Input::Interrupt(JobId("first".into())), 1, None).output,
        Output::Interrupting(Interrupting::Interrupted(JobId("first".into())))
    );
    start(&memory, "second", 1);
    execute(&memory, Input::Interrupt(JobId("first".into())), 2, None);
    assert_eq!(
        memory.inspect().unwrap().job_vector[1].state,
        State::Recording
    );
    execute(&memory, Input::Drop, 3, None);
    let state = memory.inspect().unwrap();
    assert!(
        memory
            .execute(request(
                Input::Turn(Options {
                    delivery_option: None,
                    backend_name_vector_option: None
                }),
                4,
                Some("first")
            ))
            .is_err()
    );
    assert_eq!(memory.inspect().unwrap(), state);
    execute(&memory, Input::Apply(Set::Order(Vec::new())), 4, None);
    assert_eq!(
        execute(
            &memory,
            Input::Turn(Options {
                delivery_option: None,
                backend_name_vector_option: None
            }),
            4,
            Some("third")
        )
        .output,
        Output::Turning(Turning::NoBackend)
    );
}

#[test]
fn restart_returns_only_the_accepted_work_plan_and_normalizes_recording() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsReady))
        .unwrap();
    start(&memory, "transcribing", 0);
    stop(&memory, "transcribing");
    start(&memory, "waiting", 1);
    stop(&memory, "waiting");
    heard(&memory, "waiting", "ready");
    start(&memory, "recording", 2);
    drop(memory);
    let reopened = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let result = execute(&reopened, Input::Recover, 3, None);
    let Output::Recovered(recovery) = result.output else {
        panic!()
    };
    assert_eq!(
        recovery.hear.0,
        vec![JobId("transcribing".into()), JobId("recording".into())]
    );
    assert_eq!(ready_id(recovery.next), "waiting");
    assert_eq!(
        reopened.inspect().unwrap().job_vector[2].state,
        State::Transcribing
    );
    assert_eq!(
        result.changes.job_state_vector,
        vec![JobState {
            job_id: JobId("recording".into()),
            state: State::Transcribing
        }]
    );
}

#[test]
fn kernel_rollback_and_schema_mismatch_are_real_persistence_boundaries() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("kernel.redb");
    let table: memory::Table<&str, Text> = memory::Table::new("fixture");
    let kernel = memory::Memory::open_with_schema(
        &path,
        &memory::Schema {
            version: memory::SchemaVersion::new(1),
        },
    )
    .unwrap();
    kernel
        .write(|transaction| table.insert(transaction, "value", &Text("committed".into())))
        .unwrap();
    let failed: memory::Result<()> = kernel.write(|transaction| {
        table.insert(transaction, "value", &Text("aborted".into()))?;
        Err(std::io::Error::other("fixture rollback").into())
    });
    assert!(failed.is_err());
    drop(kernel);
    let kernel = memory::Memory::open_with_schema(
        &path,
        &memory::Schema {
            version: memory::SchemaVersion::new(1),
        },
    )
    .unwrap();
    assert_eq!(
        kernel
            .read(|transaction| table.get(transaction, "value"))
            .unwrap(),
        Some(Text("committed".into()))
    );
    drop(kernel);
    assert!(
        memory::Memory::open_with_schema(
            &path,
            &memory::Schema {
                version: memory::SchemaVersion::new(2)
            }
        )
        .is_err()
    );
}

#[test]
fn actual_concurrent_calls_are_serialized_and_keep_every_import() {
    let directory = TempDir::new().unwrap();
    let memory = std::sync::Arc::new(
        definition(&directory)
            .open(configuration(Sequence::AsRecorded))
            .unwrap(),
    );
    let handles: Vec<_> = (0..8)
        .map(|index| {
            let memory = memory.clone();
            std::thread::spawn(move || {
                execute(
                    &memory,
                    Input::Import(Import_Data {
                        moment: Moment(index),
                        text: Text(index.to_string()),
                    }),
                    index,
                    Some(&format!("job-{index}")),
                )
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let state = memory.inspect().unwrap();
    assert_eq!(state.job_vector.len(), 8);
    let ids: std::collections::BTreeSet<_> = state
        .job_vector
        .iter()
        .map(|job| job.job_id.0.as_str())
        .collect();
    assert_eq!(ids.len(), 8);
}

#[test]
fn archive_round_trip_keeps_the_recording_snapshot_and_both_unobserved_cases() {
    let settings = configuration(Sequence::AsReady).recording_settings;
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&settings).unwrap();
    assert_eq!(
        rkyv::from_bytes::<RecordingSettings, rkyv::rancor::Error>(&bytes).unwrap(),
        settings
    );
    for unobserved in [
        Failing::Unobserved(nexus::Unobserved::Unsubmitted),
        Failing::Unobserved(nexus::Unobserved::Unanswered),
    ] {
        let attempt = Try {
            backend_name: BackendName("speech".into()),
            failing: unobserved,
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&attempt).unwrap();
        assert_eq!(
            rkyv::from_bytes::<Try, rkyv::rancor::Error>(&bytes).unwrap(),
            attempt
        );
    }
}

#[test]
fn invalid_recording_settings_are_atomic_and_the_final_state_is_durable() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let before = memory.inspect().unwrap();
    let mut invalid = before.configuration.recording_settings.clone();
    invalid.sample_rate = SampleRate(12345);
    assert!(
        memory
            .execute(request(
                Input::Apply(Set::RecordingSettings(invalid)),
                0,
                None
            ))
            .is_err()
    );
    assert_eq!(memory.inspect().unwrap(), before);
    let mut invalid = before.configuration.recording_settings.clone();
    invalid.capture_fragment_bytes = CaptureFragmentBytes(3);
    assert!(
        memory
            .execute(request(
                Input::Apply(Set::RecordingSettings(invalid)),
                0,
                None
            ))
            .is_err()
    );
    assert_eq!(memory.inspect().unwrap(), before);
    drop(memory);
    assert_eq!(
        definition(&directory)
            .open(configuration(Sequence::AsReady))
            .unwrap()
            .inspect()
            .unwrap(),
        before
    );
}

#[test]
fn every_generated_ordinary_input_uses_its_paired_answering_capability() {
    use futures_util::{FutureExt, StreamExt};
    use nexus::{Capacity, Publisher, Publishing};
    use signal::{Following, Frame};
    use signal_listener::{Answer, Answerable, Answering, Input, Output};

    struct Scenario {
        input: Input,
        expected: Output,
    }

    #[derive(Clone)]
    struct Jobs(Vec<JobState>);

    impl Following for Jobs {
        type Change = JobState;

        fn after(mut state: Self, change: JobState) -> Self {
            state.0.retain(|job| job.job_id != change.job_id);
            state.0.push(change);
            state
        }
    }
    struct Fixture {
        id: JobId,
        text: Text,
        jobs: Publisher<Jobs, JobState>,
    }
    impl Answering for Fixture {
        fn toggle(&self) -> Result<signal_listener::Toggling, signal_listener::Output> {
            Ok(signal_listener::Toggling::Began(self.id.clone()))
        }
        fn toggle_custom(
            &self,
            _: &Options,
        ) -> Result<signal_listener::Toggling, signal_listener::Output> {
            Ok(signal_listener::Toggling::Ended(self.id.clone()))
        }
        fn cancel(&self) -> Result<JobId, signal_listener::Output> {
            Ok(self.id.clone())
        }
        fn recall(&self) -> Result<Text, signal_listener::Output> {
            Ok(self.text.clone())
        }
        fn recall_job(&self, _: &JobId) -> Result<Text, signal_listener::Output> {
            Ok(self.text.clone())
        }
        fn retranscribe(&self, _: &JobId) -> Result<JobId, signal_listener::Output> {
            Ok(self.id.clone())
        }
        fn jobs(&self) -> Result<Vec<JobState>, signal_listener::Output> {
            Ok(Vec::new())
        }
        fn history(&self) -> Result<Vec<Past>, signal_listener::Output> {
            Ok(Vec::new())
        }
        fn subscribe(
            &self,
        ) -> Result<signal::Stream<Vec<JobState>, JobState>, signal_listener::Output> {
            let frames = self.jobs.open().map(|frame| match frame {
                Frame::Opened(jobs) => Frame::Opened(jobs.0),
                Frame::Changed(job) => Frame::Changed(job),
                Frame::Lagged(jobs) => Frame::Lagged(jobs.0),
            });
            let frames: std::pin::Pin<Box<dyn futures_util::Stream<Item = _> + Send>> =
                Box::pin(frames);
            Ok(signal::Stream::from(frames))
        }
    }
    let fixture = Fixture {
        id: JobId("paired".into()),
        text: Text("paired-text".into()),
        jobs: Publisher::create(Jobs(Vec::new()), Capacity(2)).unwrap(),
    };
    let scenarios = [
        Scenario {
            input: Input::Toggle,
            expected: Output::Toggled(signal_listener::Toggling::Began(fixture.id.clone())),
        },
        Scenario {
            input: Input::ToggleCustom(Options {
                delivery_option: None,
                backend_name_vector_option: None,
            }),
            expected: Output::CustomToggled(signal_listener::Toggling::Ended(fixture.id.clone())),
        },
        Scenario {
            input: Input::Cancel,
            expected: Output::Cancelled(fixture.id.clone()),
        },
        Scenario {
            input: Input::Recall,
            expected: Output::Recalled(fixture.text.clone()),
        },
        Scenario {
            input: Input::RecallJob(fixture.id.clone()),
            expected: Output::JobRecalled(fixture.text.clone()),
        },
        Scenario {
            input: Input::Retranscribe(fixture.id.clone()),
            expected: Output::Retranscribing(fixture.id.clone()),
        },
        Scenario {
            input: Input::Jobs,
            expected: Output::JobsListed(Vec::new()),
        },
        Scenario {
            input: Input::History,
            expected: Output::HistoryListed(Vec::new()),
        },
    ];
    for scenario in scenarios {
        let Answer::Once(output) = scenario.input.answer(&fixture) else {
            panic!("one-shot input yielded a stream")
        };
        assert_eq!(output, scenario.expected);
    }
    let Answer::Subscribed(mut stream) = Input::Subscribe.answer(&fixture) else {
        panic!("Subscribe yielded a one-shot result")
    };
    assert_eq!(
        stream.next().now_or_never(),
        Some(Some(Frame::Opened(Vec::new())))
    );

    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .open(configuration(Sequence::AsRecorded))
        .unwrap();
    let committed = execute(
        &memory,
        listener_memory::operations::Input::Import(Import_Data {
            moment: Moment(17),
            text: fixture.text.clone(),
        }),
        17,
        Some(&fixture.id.0),
    );
    let [change] = &committed.changes.job_state_vector[..] else {
        panic!("import must return one committed change")
    };
    fixture.jobs.publish(change.clone());
    assert_eq!(
        stream.next().now_or_never(),
        Some(Some(Frame::Changed(change.clone())))
    );

    let frame = Output::Subscribed(Frame::Changed(change.clone()));
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&frame).unwrap();
    assert_eq!(
        rkyv::from_bytes::<Output, rkyv::rancor::Error>(&bytes).unwrap(),
        frame
    );
}

#[test]
fn meta_configuration_is_a_unit_query_and_its_paired_dispatch_is_generated() {
    use meta_signal_listener::{Answer, Answerable, Answering};
    struct Fixture {
        configuration: Configuration,
    }
    impl Answering for Fixture {
        fn configure(&self, set: &Set) -> Result<Set, meta_signal_listener::Output> {
            Ok(set.clone())
        }
        fn configuration(&self) -> Result<Configuration, meta_signal_listener::Output> {
            Ok(self.configuration.clone())
        }
        fn remember(
            &self,
            _: &meta_signal_listener::Remember_Data,
        ) -> Result<JobId, meta_signal_listener::Output> {
            Ok(JobId("imported".into()))
        }
        fn subscribe(
            &self,
        ) -> Result<signal::Stream<Configuration, Set>, meta_signal_listener::Output> {
            Err(meta_signal_listener::Output::Refused(
                meta_signal_listener::UnknownBackend(BackendName("missing".into())),
            ))
        }
    }
    let fixture = Fixture {
        configuration: configuration(Sequence::AsRecorded),
    };
    let Answer::Once(output) = meta_signal_listener::Input::Configuration.answer(&fixture) else {
        panic!()
    };
    assert_eq!(
        output,
        meta_signal_listener::Output::Configuration(fixture.configuration)
    );
    let bytes =
        rkyv::to_bytes::<rkyv::rancor::Error>(&meta_signal_listener::Input::Configuration).unwrap();
    assert_eq!(
        rkyv::from_bytes::<meta_signal_listener::Input, rkyv::rancor::Error>(&bytes).unwrap(),
        meta_signal_listener::Input::Configuration
    );
}

#[cfg(feature = "datom")]
#[test]
fn datom_queries_and_refusals_keep_the_approved_shapes() {
    use datom_codec::{Actualizing, Budget, Potential};
    let budget = || Budget {
        remaining: 128,
        reader: 128_i64.try_into().unwrap(),
        depth: 0,
        maximum_depth: 16,
    };
    let query: meta_signal_listener::Input = Potential::from("Configuration")
        .actualize(&mut budget())
        .unwrap();
    assert_eq!(query, meta_signal_listener::Input::Configuration);
    let refusal: meta_signal_listener::Output = Potential::from("Refused.missing")
        .actualize(&mut budget())
        .unwrap();
    assert_eq!(
        refusal,
        meta_signal_listener::Output::Refused(meta_signal_listener::UnknownBackend(BackendName(
            "missing".into()
        )))
    );
    let ordinary: signal_listener::Output = Potential::from("Toggled.Began.job")
        .actualize(&mut budget())
        .unwrap();
    assert_eq!(
        ordinary,
        signal_listener::Output::Toggled(signal_listener::Toggling::Began(JobId("job".into())))
    );
}
