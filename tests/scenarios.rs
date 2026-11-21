use listener_library::*;
use listener_memory::{
    ExecutingMemory, InspectingMemory, MemoryDefinition, OpeningMemory, operations::*, state::Audio,
};
use meta_signal_listener::Set;
use tempfile::TempDir;

// These constructors are test fixtures, not production entry points or clocks.
fn configuration(sequence: Sequence) -> Configuration {
    Configuration {
        backend_vector: vec![backend("speech", "model")],
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
fn backend(name: &str, model: &str) -> Backend {
    Backend {
        backend_name: BackendName(name.into()),
        protocol: Protocol::OpenAiTranscription,
        service_address: ServiceAddress("https://fixture.invalid".into()),
        model_name: ModelName(model.into()),
        secret_entry: SecretEntry("fixture/key".into()),
        language: Language("en".into()),
        instruction: Instruction("transcribe".into()),
        timeout: Timeout(Seconds(30)),
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
fn hearing(id: &str, text: &str) -> Input {
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
    })
}
fn heard(memory: &listener_memory::Memory, id: &str, text: &str) -> Next {
    let result = execute(memory, hearing(id, text), 0, None);
    let Output::Next(next) = result.output else {
        panic!("next expected")
    };
    next
}
fn finishing(id: &str) -> Input {
    Input::Finish(Finish_Data {
        job_id: JobId(id.into()),
        handing: Handing::Kept,
    })
}
fn finish(memory: &listener_memory::Memory, id: &str) -> Next {
    let Output::Next(next) = execute(memory, finishing(id), 0, None).output else {
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
fn open(directory: &TempDir, sequence: Sequence) -> listener_memory::Memory {
    definition(directory).open(configuration(sequence)).unwrap()
}
fn turn() -> Input {
    Input::Turn(Options {
        delivery_option: None,
        backend_name_vector_option: None,
    })
}
fn ready(id: &str, text: &str, delivery: Delivery) -> Ready {
    Ready {
        job_id: JobId(id.into()),
        text: Text(text.into()),
        delivery,
    }
}
fn past(id: &str, moment: i64, text: &str) -> Past {
    Past {
        job_id: JobId(id.into()),
        moment: Moment(moment),
        text: Text(text.into()),
    }
}
fn term(text: &str) -> Term {
    Term(text.into())
}
fn location(directory: &str, id: &str) -> AudioLocation {
    AudioLocation {
        directory: Directory(directory.into()),
        job_id: JobId(id.into()),
    }
}
fn job<'record>(
    record: &'record listener_memory::state::Record,
    id: &str,
) -> &'record listener_memory::state::Job {
    record
        .job_vector
        .iter()
        .find(|job| job.job_id.0 == id)
        .expect("job present")
}
// The exact change record a commit is expected to report; no Set unless one applied.
fn changes(states: &[(&str, State)], retired: &[&str]) -> Changes {
    Changes {
        job_state_vector: states
            .iter()
            .map(|(id, state)| JobState {
                job_id: JobId((*id).into()),
                state: state.clone(),
            })
            .collect(),
        retired: Retired(retired.iter().map(|id| JobId((*id).into())).collect()),
        set_option: None,
    }
}
fn changed(
    memory: &listener_memory::Memory,
    input: Input,
    moment: i64,
    id: Option<&str>,
    expected: Changes,
) -> Output {
    let committed = execute(memory, input, moment, id);
    assert_eq!(committed.changes, expected);
    committed.output
}
// A query or refusal: no reported change and no altered durable state.
fn observe(memory: &listener_memory::Memory, input: Input, moment: i64) -> Output {
    let before = memory.inspect().unwrap();
    let output = changed(memory, input, moment, None, changes(&[], &[]));
    assert_eq!(memory.inspect().unwrap(), before);
    output
}
fn apply(memory: &listener_memory::Memory, set: Set) {
    let committed = execute(memory, Input::Apply(set.clone()), 0, None);
    assert_eq!(
        committed,
        Committed {
            output: Output::Applying(Applying::Applied(set.clone())),
            changes: Changes {
                set_option: Some(set),
                ..changes(&[], &[])
            },
        }
    );
}
fn settings(memory: &listener_memory::Memory) -> Configuration {
    let Output::Configuration(configuration) = observe(memory, Input::Settings, 0) else {
        panic!("configuration expected")
    };
    configuration
}
fn import(memory: &listener_memory::Memory, id: &str, moment: i64, text: &str) {
    let committed = execute(
        memory,
        Input::Import(Import_Data {
            moment: Moment(moment),
            text: Text(text.into()),
        }),
        moment,
        Some(id),
    );
    assert_eq!(committed.output, Output::Imported(JobId(id.into())));
    assert_eq!(committed.changes, changes(&[(id, State::Delivered)], &[]));
}
// Records, hears and delivers one job; callers leave no other job waiting before it.
fn capture(memory: &listener_memory::Memory, id: &str, moment: i64, text: &str) -> Fresh_Data {
    let fresh = start(memory, id, moment);
    stop(memory, id);
    assert_eq!(ready_id(heard(memory, id, text)), id);
    assert_eq!(finish(memory, id), Next::Nothing);
    fresh
}
fn exhaust(memory: &listener_memory::Memory, id: &str, tries: Vec<Try>) -> Committed {
    execute(
        memory,
        Input::Record(Record_Data {
            job_id: JobId(id.into()),
            hearing: Hearing::Exhausted(tries),
        }),
        0,
        None,
    )
}
#[cfg(feature = "datom")]
fn decode<T: datom_codec::Composing>(text: &str) -> Result<T, datom_codec::Error> {
    use datom_codec::{Actualizing, Budget, Potential};
    Potential::<T>::from(text).actualize(&mut Budget {
        remaining: 1024,
        reader: 4096_usize.try_into().unwrap(),
        depth: 0,
        maximum_depth: 64,
    })
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
        reader: 128_usize.try_into().unwrap(),
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

// RC13 coverage: Last. Catches a Last that answers by insertion order or by the first job,
// that answers the newest job even when it holds no text, that keeps a requeued job's first
// transcript, or that still answers text TextKept has pruned instead of NothingKept.
#[test]
fn last_answers_the_newest_retained_text_and_nothing_kept_once_none_remains() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    assert_eq!(
        observe(&memory, Input::Last, 0),
        Output::Lasting(Lasting::NothingKept)
    );
    import(&memory, "middle", 200, "middle text");
    import(&memory, "newer", 300, "newer text");
    capture(&memory, "oldest", 100, "oldest text");
    let newer = Output::Lasting(Lasting::Ready(ready("newer", "newer text", Delivery::Kept)));
    assert_eq!(observe(&memory, Input::Last, 400), newer);
    start(&memory, "failed", 400);
    stop(&memory, "failed");
    exhaust(&memory, "failed", Vec::new());
    assert_eq!(observe(&memory, Input::Last, 400), newer);
    capture(&memory, "retaken", 500, "first take");
    assert_eq!(
        execute(&memory, Input::Requeue(JobId("retaken".into())), 600, None).output,
        Output::Requeueing(Requeueing::Requeued(JobId("retaken".into())))
    );
    assert_eq!(
        ready_id(heard(&memory, "retaken", "second take")),
        "retaken"
    );
    assert_eq!(finish(&memory, "retaken"), Next::Nothing);
    let retaken = Output::Lasting(Lasting::Ready(ready(
        "retaken",
        "second take",
        Delivery::Kept,
    )));
    assert_eq!(observe(&memory, Input::Last, 600), retaken);
    apply(&memory, Set::TextKept(Retention::Days(Days(1))));
    execute(&memory, Input::Prune, 86_400 + 400, None);
    assert_eq!(observe(&memory, Input::Last, 86_400 + 400), retaken);
    execute(&memory, Input::Prune, 86_400 + 500, None);
    assert_eq!(
        observe(&memory, Input::Last, 86_400 + 500),
        Output::Lasting(Lasting::NothingKept)
    );
    assert!(!memory.inspect().unwrap().job_vector.is_empty());
}

// RC13 coverage: Recollect. Catches oldest-first or insertion-order history, truncation
// before sorting (which keeps the oldest), an ignored or off-by-one HistoryLength, history
// that omits imported or captured text, that lists a job without text, or that still lists
// text past TextKept.
#[test]
fn recollect_lists_retained_imported_and_captured_text_newest_first_within_history_length() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    assert_eq!(
        observe(&memory, Input::Recollect, 0),
        Output::Recollected(Vec::new())
    );
    import(&memory, "old", 100, "old text");
    capture(&memory, "spoken", 300, "spoken text");
    import(&memory, "recent", 400, "recent text");
    import(&memory, "older", 200, "older text");
    start(&memory, "silent", 500);
    stop(&memory, "silent");
    exhaust(&memory, "silent", Vec::new());
    let history = vec![
        past("recent", 400, "recent text"),
        past("spoken", 300, "spoken text"),
        past("older", 200, "older text"),
        past("old", 100, "old text"),
    ];
    assert_eq!(
        observe(&memory, Input::Recollect, 600),
        Output::Recollected(history.clone())
    );
    apply(&memory, Set::HistoryLength(HistoryLength(2)));
    assert_eq!(
        observe(&memory, Input::Recollect, 600),
        Output::Recollected(history[..2].to_vec())
    );
    apply(&memory, Set::HistoryLength(HistoryLength(0)));
    assert_eq!(
        observe(&memory, Input::Recollect, 600),
        Output::Recollected(Vec::new())
    );
    apply(&memory, Set::HistoryLength(HistoryLength(20)));
    apply(&memory, Set::TextKept(Retention::Days(Days(1))));
    changed(
        &memory,
        Input::Prune,
        86_400 + 100,
        None,
        changes(&[], &["old"]),
    );
    assert_eq!(
        observe(&memory, Input::Recollect, 86_400 + 100),
        Output::Recollected(history[..3].to_vec())
    );
    changed(
        &memory,
        Input::Prune,
        86_400 + 300,
        None,
        changes(&[], &["older"]),
    );
    assert_eq!(
        observe(&memory, Input::Recollect, 86_400 + 300),
        Output::Recollected(history[..1].to_vec())
    );
    assert_eq!(
        observe(&memory, Input::Find(JobId("spoken".into())), 86_400 + 300),
        Output::Finding(Finding::NothingKept)
    );
}

// RC13 coverage: Backend settings. Catches Set.Backend appending a second definition under an
// existing name or moving a replaced one to the end, an inserted backend joining Order on its
// own, RemoveBackend of an absent name reported as applied or committing a change, and
// backend definitions or Order lost on reopen.
#[test]
fn backend_settings_insert_replace_and_remove_by_name_and_survive_reopen() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    let speech = BackendName("speech".into());
    let whisper = BackendName("whisper".into());
    apply(&memory, Set::Backend(backend("whisper", "whisper-one")));
    let configuration = settings(&memory);
    assert_eq!(
        configuration.backend_vector,
        vec![
            backend("speech", "model"),
            backend("whisper", "whisper-one")
        ]
    );
    assert_eq!(configuration.order, Order(vec![speech.clone()]));
    apply(&memory, Set::Backend(backend("speech", "model-two")));
    let expected = vec![
        backend("speech", "model-two"),
        backend("whisper", "whisper-one"),
    ];
    assert_eq!(settings(&memory).backend_vector, expected);
    apply(&memory, Set::Backend(backend("spare", "spare-one")));
    apply(&memory, Set::RemoveBackend(BackendName("spare".into())));
    assert_eq!(settings(&memory).backend_vector, expected);
    assert_eq!(
        observe(
            &memory,
            Input::Apply(Set::RemoveBackend(BackendName("spare".into()))),
            0
        ),
        Output::Applying(Applying::UnknownBackend(
            meta_signal_listener::UnknownBackend(BackendName("spare".into()))
        ))
    );
    apply(&memory, Set::Order(vec![whisper.clone(), speech.clone()]));
    let state = memory.inspect().unwrap();
    drop(memory);
    let reopened = open(&directory, Sequence::AsReady);
    assert_eq!(reopened.inspect().unwrap(), state);
    let configuration = settings(&reopened);
    assert_eq!(configuration.backend_vector, expected);
    assert_eq!(configuration.order, Order(vec![whisper, speech]));
}

// RC13 coverage: AddTerm and RemoveTerm. Catches case folding or trimming of a term's
// spelling, sorted or set-ordered terms instead of insertion order, a repeated AddTerm that
// duplicates, a RemoveTerm of an absent or differently cased term that refuses or removes
// another, Tries answering stale terms, and terms lost on reopen.
#[test]
fn terms_keep_spelling_and_order_and_add_and_remove_are_idempotent_across_reopen() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    start(&memory, "job", 1);
    for text in ["Ethos Zero", "datom", "Datom"] {
        apply(&memory, Set::AddTerm(term(text)));
    }
    let added = vec![term("Ethos Zero"), term("datom"), term("Datom")];
    assert_eq!(settings(&memory).term_vector, added);
    let before = memory.inspect().unwrap();
    apply(&memory, Set::AddTerm(term("datom")));
    assert_eq!(memory.inspect().unwrap(), before);
    apply(&memory, Set::RemoveTerm(term("datom")));
    let kept = vec![term("Ethos Zero"), term("Datom")];
    assert_eq!(settings(&memory).term_vector, kept);
    let before = memory.inspect().unwrap();
    apply(&memory, Set::RemoveTerm(term("datom")));
    apply(&memory, Set::RemoveTerm(term("ethos zero")));
    assert_eq!(memory.inspect().unwrap(), before);
    assert_eq!(
        observe(&memory, Input::Tries(JobId("job".into())), 2),
        Output::Trial(Trial_Data {
            backend_vector: vec![backend("speech", "model")],
            term_vector: kept.clone(),
        })
    );
    drop(memory);
    let reopened = open(&directory, Sequence::AsReady);
    assert_eq!(reopened.inspect().unwrap(), before);
    assert_eq!(settings(&reopened).term_vector, kept);
}

// RC13 coverage: explicit overrides. Catches a Turn that ignores or reorders the explicit
// backend order or delivery, a job that rereads Order or Delivery from the current
// configuration at hearing or delivery time, Tries resolving the configured order instead of
// the job's snapshot, an unknown override name that still starts a recording or commits a
// change, and snapshots lost on reopen.
#[test]
fn turn_overrides_are_snapshotted_against_later_configuration_and_survive_reopen() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    let speech = BackendName("speech".into());
    let whisper = BackendName("whisper".into());
    apply(&memory, Set::Backend(backend("whisper", "whisper-one")));
    let custom_order = vec![whisper.clone(), speech.clone()];
    let output = changed(
        &memory,
        Input::Turn(Options {
            delivery_option: Some(Delivery::Copied),
            backend_name_vector_option: Some(custom_order.clone()),
        }),
        1,
        Some("custom"),
        changes(&[("custom", State::Recording)], &[]),
    );
    assert!(matches!(output, Output::Turning(Turning::Fresh(_))));
    stop(&memory, "custom");
    apply(&memory, Set::Delivery(Delivery::Pasted));
    apply(&memory, Set::Order(vec![speech.clone(), whisper.clone()]));
    apply(&memory, Set::Backend(backend("speech", "model-two")));
    assert_eq!(
        observe(&memory, Input::Tries(JobId("custom".into())), 2),
        Output::Trial(Trial_Data {
            backend_vector: vec![
                backend("whisper", "whisper-one"),
                backend("speech", "model-two")
            ],
            term_vector: Vec::new(),
        })
    );
    start(&memory, "plain", 2);
    stop(&memory, "plain");
    let state = memory.inspect().unwrap();
    let custom = job(&state, "custom");
    assert_eq!(custom.delivery, Delivery::Copied);
    assert_eq!(custom.backend_name_vector, custom_order);
    let plain = job(&state, "plain");
    assert_eq!(plain.delivery, Delivery::Pasted);
    assert_eq!(plain.backend_name_vector, vec![speech, whisper]);
    assert_eq!(
        heard(&memory, "custom", "custom text"),
        Next::Deliverable(ready("custom", "custom text", Delivery::Copied))
    );
    assert_eq!(finish(&memory, "custom"), Next::Nothing);
    assert_eq!(
        heard(&memory, "plain", "plain text"),
        Next::Deliverable(ready("plain", "plain text", Delivery::Pasted))
    );
    assert_eq!(finish(&memory, "plain"), Next::Nothing);
    let before = memory.inspect().unwrap();
    // The refusal's shape awaits a ruling (RC4); only its effect is held here.
    if let Ok(committed) = memory.execute(request(
        Input::Turn(Options {
            delivery_option: None,
            backend_name_vector_option: Some(vec![BackendName("missing".into())]),
        }),
        3,
        Some("unknown"),
    )) {
        assert!(!matches!(
            committed.output,
            Output::Turning(Turning::Fresh(_))
        ));
        assert_eq!(committed.changes, changes(&[], &[]));
    }
    assert_eq!(memory.inspect().unwrap(), before);
    drop(memory);
    assert_eq!(
        open(&directory, Sequence::AsReady).inspect().unwrap(),
        before
    );
}

// RC13 coverage: AudioLocation archive identity. Catches a location rebuilt from the current
// AudioDirectory (at Prune or after reopen), an archive that drops or rewrites the directory,
// Prune marking audio erased itself, and Forget matching on the job identity alone.
#[test]
fn audio_location_archives_with_its_original_directory_across_reopen() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    let fresh = capture(&memory, "first", 0, "one");
    let original = location("/fixture/audio-before", "first");
    assert_eq!(fresh.audio_location, original);
    let turning = Output::Turning(Turning::Fresh(fresh));
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&turning).unwrap();
    assert_eq!(
        rkyv::from_bytes::<Output, rkyv::rancor::Error>(&bytes).unwrap(),
        turning
    );
    apply(
        &memory,
        Set::AudioDirectory(Directory("/fixture/audio-after".into())),
    );
    let later = start(&memory, "second", 1);
    assert_eq!(
        later.audio_location,
        location("/fixture/audio-after", "second")
    );
    drop(memory);
    let memory = open(&directory, Sequence::AsRecorded);
    apply(&memory, Set::AudioKept(Retention::Days(Days(0))));
    let pruned = observe(&memory, Input::Prune, 10);
    assert_eq!(pruned, Output::Pruned(vec![original.clone()]));
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&pruned).unwrap();
    let Output::Pruned(restored) = rkyv::from_bytes::<Output, rkyv::rancor::Error>(&bytes).unwrap()
    else {
        panic!("pruned locations expected")
    };
    assert_eq!(restored, vec![original]);
    assert_eq!(
        observe(
            &memory,
            Input::Forget(vec![location("/fixture/audio-after", "first")]),
            10
        ),
        Output::Forgotten
    );
    changed(
        &memory,
        Input::Forget(restored),
        10,
        None,
        changes(&[], &[]),
    );
    let state = memory.inspect().unwrap();
    assert_eq!(job(&state, "first").audio, Audio::Erased);
    assert_eq!(
        job(&state, "second").audio,
        Audio::Kept(Directory("/fixture/audio-after".into()))
    );
}

// RC13 coverage: Exhausted under both policies. Catches AsRecorded treating a Failed job as
// still blocking later text (a wedged queue), AsReady holding ready text behind an earlier
// job, an exhausted job offered for delivery or keeping text, exhaustion releasing or
// stealing another job's delivery claim, lost tries, and extra reported changes.
#[test]
fn exhaustion_frees_the_delivery_order_under_both_sequence_policies() {
    for sequence in [Sequence::AsRecorded, Sequence::AsReady] {
        let directory = TempDir::new().unwrap();
        let memory = open(&directory, sequence.clone());
        for (id, moment) in [("first", 1), ("second", 2), ("third", 3)] {
            start(&memory, id, moment);
            stop(&memory, id);
        }
        let tries = vec![Try {
            backend_name: BackendName("speech".into()),
            failing: Failing::Failure(Failure("timeout".into())),
        }];
        let first = exhaust(&memory, "first", tries.clone());
        assert_eq!(first.output, Output::Next(Next::Nothing));
        assert_eq!(first.changes, changes(&[("first", State::Failed)], &[]));
        let third = heard(&memory, "third", "three");
        let second = exhaust(&memory, "second", Vec::new());
        assert_eq!(second.changes, changes(&[("second", State::Failed)], &[]));
        let Output::Next(second) = second.output else {
            panic!("next expected")
        };
        let delivered = Next::Deliverable(ready("third", "three", Delivery::Kept));
        match sequence {
            Sequence::AsRecorded => {
                assert_eq!(third, Next::Nothing);
                assert_eq!(second, delivered);
            }
            Sequence::AsReady => {
                assert_eq!(third, delivered);
                assert_eq!(second, Next::Nothing);
            }
        }
        assert_eq!(finish(&memory, "third"), Next::Nothing);
        let state = memory.inspect().unwrap();
        assert_eq!(job(&state, "first").state, State::Failed);
        assert_eq!(job(&state, "first").try_vector, tries);
        assert!(job(&state, "first").transcript_vector.is_empty());
        assert_eq!(job(&state, "second").state, State::Failed);
        assert_eq!(job(&state, "third").state, State::Delivered);
        assert_eq!(state.delivery_claim.0, None);
        for id in ["first", "second"] {
            assert_eq!(
                observe(&memory, Input::Find(JobId(id.into())), 4),
                Output::Finding(Finding::NothingKept)
            );
        }
        assert_eq!(
            observe(&memory, Input::Last, 4),
            Output::Lasting(Lasting::Ready(ready("third", "three", Delivery::Kept)))
        );
    }
}

// RC13 coverage: committed and no-change outcomes. Catches Changes that list every job
// instead of those whose state moved, a query or refusal that writes or reports a change,
// Prune erasing audio itself, Recover reporting a job already Transcribing, a retirement
// also reported as a state change, a repeated Forget reporting a second retirement, and an
// audio-only erasure reported as a job state change.
#[test]
fn every_commit_reports_exactly_its_state_changes_and_queries_change_nothing() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    for input in [
        Input::Last,
        Input::Pending,
        Input::Recollect,
        Input::Settings,
        Input::Prune,
        Input::Recover,
        Input::Drop,
        Input::Find(JobId("missing".into())),
        Input::Interrupt(JobId("missing".into())),
        Input::Requeue(JobId("missing".into())),
    ] {
        observe(&memory, input, 0);
    }
    let recording = |id| changes(&[(id, State::Recording)], &[]);
    changed(&memory, turn(), 1, Some("a"), recording("a"));
    changed(
        &memory,
        turn(),
        1,
        None,
        changes(&[("a", State::Transcribing)], &[]),
    );
    changed(&memory, turn(), 2, Some("b"), recording("b"));
    assert_eq!(
        changed(
            &memory,
            hearing("a", "one"),
            2,
            None,
            changes(&[("a", State::Waiting)], &[])
        ),
        Output::Next(Next::Deliverable(ready("a", "one", Delivery::Kept)))
    );
    for input in [
        Input::Find(JobId("a".into())),
        Input::Tries(JobId("a".into())),
        Input::Interrupt(JobId("a".into())),
        Input::Pending,
    ] {
        observe(&memory, input, 2);
    }
    changed(
        &memory,
        Input::Drop,
        3,
        None,
        changes(&[("b", State::Cancelled)], &[]),
    );
    changed(
        &memory,
        finishing("a"),
        3,
        None,
        changes(&[("a", State::Delivered)], &[]),
    );
    changed(
        &memory,
        Input::Requeue(JobId("a".into())),
        4,
        None,
        changes(&[("a", State::Transcribing)], &[]),
    );
    observe(&memory, Input::Recover, 4);
    changed(
        &memory,
        hearing("a", "two"),
        5,
        None,
        changes(&[("a", State::Waiting)], &[]),
    );
    changed(
        &memory,
        finishing("a"),
        5,
        None,
        changes(&[("a", State::Delivered)], &[]),
    );
    apply(&memory, Set::AudioKept(Retention::Days(Days(0))));
    observe(
        &memory,
        Input::Apply(Set::Order(vec![BackendName("missing".into())])),
        6,
    );
    let first = location("/fixture/audio-before", "a");
    let second = location("/fixture/audio-before", "b");
    assert_eq!(
        observe(&memory, Input::Prune, 10),
        Output::Pruned(vec![first.clone(), second.clone()])
    );
    changed(
        &memory,
        Input::Forget(vec![second.clone()]),
        10,
        None,
        changes(&[], &["b"]),
    );
    observe(&memory, Input::Forget(vec![second]), 10);
    changed(
        &memory,
        Input::Forget(vec![first]),
        10,
        None,
        changes(&[], &[]),
    );
    let state = memory.inspect().unwrap();
    assert_eq!(job(&state, "a").audio, Audio::Erased);
    assert_eq!(
        observe(&memory, Input::Prune, 10),
        Output::Pruned(Vec::new())
    );
    drop(memory);
    assert_eq!(
        open(&directory, Sequence::AsReady).inspect().unwrap(),
        state
    );
}

// RC13 coverage: Jobs stream projection, restricted to active jobs until the membership ruling
// (RC11). Catches a commit that moves an active job without reporting it (Interrupt, Requeue
// back to Transcribing, Recover's normalization after reopen), a Pending query that omits
// Waiting or interrupted jobs or lists a cancelled one, and a retirement left in the view.
#[test]
fn committed_changes_fold_to_the_pending_query_for_active_jobs() {
    struct View {
        jobs: std::collections::BTreeMap<String, State>,
    }
    impl View {
        fn active(
            jobs: impl IntoIterator<Item = (String, State)>,
        ) -> std::collections::BTreeMap<String, State> {
            jobs.into_iter()
                .filter(|(_, state)| {
                    !matches!(state, State::Delivered | State::Failed | State::Cancelled)
                })
                .collect()
        }
        fn follow(
            &mut self,
            memory: &listener_memory::Memory,
            input: Input,
            moment: i64,
            id: Option<&str>,
        ) {
            let committed = execute(memory, input, moment, id);
            for change in committed.changes.job_state_vector {
                self.jobs.insert(change.job_id.0, change.state);
            }
            for retired in committed.changes.retired.0 {
                self.jobs.remove(&retired.0);
            }
            let Output::Standing(pending) = observe(memory, Input::Pending, moment) else {
                panic!("pending jobs expected")
            };
            assert_eq!(
                Self::active(self.jobs.clone()),
                Self::active(pending.into_iter().map(|job| (job.job_id.0, job.state))),
                "after {:?}",
                committed.output
            );
        }
    }
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    let Output::Standing(opened) = observe(&memory, Input::Pending, 0) else {
        panic!("pending jobs expected")
    };
    let mut view = View {
        jobs: opened
            .into_iter()
            .map(|job| (job.job_id.0, job.state))
            .collect(),
    };
    view.follow(&memory, turn(), 1, Some("delivered"));
    view.follow(&memory, turn(), 1, None);
    view.follow(&memory, hearing("delivered", "text"), 1, None);
    view.follow(&memory, finishing("delivered"), 1, None);
    view.follow(&memory, turn(), 2, Some("failed"));
    view.follow(&memory, turn(), 2, None);
    view.follow(
        &memory,
        Input::Record(Record_Data {
            job_id: JobId("failed".into()),
            hearing: Hearing::Exhausted(Vec::new()),
        }),
        2,
        None,
    );
    view.follow(&memory, turn(), 3, Some("cancelled"));
    view.follow(&memory, Input::Drop, 3, None);
    view.follow(&memory, turn(), 4, Some("waiting"));
    view.follow(&memory, turn(), 4, None);
    view.follow(&memory, turn(), 5, Some("recording"));
    view.follow(&memory, hearing("waiting", "ready"), 5, None);
    view.follow(
        &memory,
        Input::Interrupt(JobId("recording".into())),
        5,
        None,
    );
    view.follow(
        &memory,
        Input::Apply(Set::AudioKept(Retention::Days(Days(0)))),
        6,
        None,
    );
    view.follow(&memory, Input::Prune, 10, None);
    view.follow(
        &memory,
        Input::Forget(vec![location("/fixture/audio-before", "cancelled")]),
        10,
        None,
    );
    assert!(!view.jobs.contains_key("cancelled"));
    view.follow(&memory, Input::Requeue(JobId("delivered".into())), 11, None);
    view.follow(&memory, turn(), 12, Some("restarted"));
    drop(memory);
    let memory = open(&directory, Sequence::AsRecorded);
    view.follow(&memory, Input::Recover, 13, None);
    assert_eq!(view.jobs.get("restarted"), Some(&State::Transcribing));
}

// RC13 coverage: AudioLocation Datom identity. Catches a Datom spelling that carries a second
// job identity beside the location, a location rebuilt from the current AudioDirectory, and a
// textual Forget that does not reach the job whose original directory it names.
#[cfg(feature = "datom")]
#[test]
fn datom_spells_an_audio_location_once_with_its_original_directory() {
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    let fresh = capture(&memory, "first", 0, "one");
    let spelled: AudioLocation = decode("{ «/fixture/audio-before» first }").unwrap();
    assert_eq!(spelled, fresh.audio_location);
    assert!(decode::<AudioLocation>("{ «/fixture/audio-before» first first }").is_err());
    apply(
        &memory,
        Set::AudioDirectory(Directory("/fixture/audio-after".into())),
    );
    apply(&memory, Set::AudioKept(Retention::Days(Days(0))));
    assert_eq!(
        decode::<Output>("Pruned.[ { «/fixture/audio-before» first } ]").unwrap(),
        observe(&memory, Input::Prune, 10)
    );
    let forget: Input = decode("Forget.[ { «/fixture/audio-before» first } ]").unwrap();
    assert_eq!(forget, Input::Forget(vec![spelled]));
    execute(&memory, forget, 10, None);
    assert_eq!(
        job(&memory.inspect().unwrap(), "first").audio,
        Audio::Erased
    );
}

// RC13 coverage: Datom spelling of term and backend settings. Catches renamed or case-folded
// Set variants, a term whose text is altered by the codec, and a Backend whose positional
// fields are read out of the declared order.
#[cfg(feature = "datom")]
#[test]
fn datom_spells_term_and_backend_settings_by_their_contract_names() {
    let cases = [
        (
            "Configure.AddTerm.«Ethos Zero»",
            Set::AddTerm(term("Ethos Zero")),
        ),
        ("Configure.RemoveTerm.Datom", Set::RemoveTerm(term("Datom"))),
        (
            "Configure.RemoveBackend.spare",
            Set::RemoveBackend(BackendName("spare".into())),
        ),
        (
            "Configure.Backend.{ whisper OpenAiTranscription «https://fixture.invalid» whisper-one «fixture/key» en transcribe 30 }",
            Set::Backend(backend("whisper", "whisper-one")),
        ),
    ];
    let directory = TempDir::new().unwrap();
    let memory = open(&directory, Sequence::AsRecorded);
    for (text, set) in cases {
        let decoded: meta_signal_listener::Input = decode(text).unwrap();
        assert_eq!(decoded, meta_signal_listener::Input::Configure(set.clone()));
        // The removal names a backend never configured; its spelling alone is held here.
        if !matches!(set, Set::RemoveBackend(_)) {
            apply(&memory, set);
        }
    }
    let configuration = settings(&memory);
    assert_eq!(configuration.term_vector, vec![term("Ethos Zero")]);
    assert_eq!(
        configuration.backend_vector,
        vec![
            backend("speech", "model"),
            backend("whisper", "whisper-one")
        ]
    );
    for misspelled in [
        "Configure.Addterm.x",
        "Configure.AddTerms.x",
        "Configure.addTerm.x",
    ] {
        assert!(decode::<meta_signal_listener::Input>(misspelled).is_err());
    }
}
