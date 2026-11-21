use listener_library::*;
use listener_memory::{
    CreatingMemory, Error, ExecutingMemory, InspectingMemory, MemoryDefinition, OpeningMemory,
    operations::*, state::Audio,
};
use meta_signal_listener::Set;
use signal_listener::JobChange;
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
// The Memory record's kernel table and key, used by fixtures that build a store by hand.
const TABLE: &str = "listener-state";
const KEY: &str = "state";
fn definition(directory: &TempDir) -> MemoryDefinition {
    MemoryDefinition {
        path: directory.path().join("new-memory.redb"),
        table_name: TABLE,
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
        result.changes.job_change_vector,
        vec![JobChange::JobState(JobState {
            job_id: JobId(id.into()),
            state: State::Recording
        })]
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
fn create(directory: &TempDir, sequence: Sequence) -> listener_memory::Memory {
    definition(directory)
        .create(configuration(sequence))
        .unwrap()
}
// Reopening takes no configuration: the persisted one is authoritative.
fn reopen(directory: &TempDir) -> listener_memory::Memory {
    definition(directory).open().unwrap()
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
// The exact change record a commit is expected to report: each changed job's new state, then
// each job that left Memory as Forgotten; no Set unless one applied.
fn changes(states: &[(&str, State)], forgotten: &[&str]) -> Changes {
    Changes {
        job_change_vector: states
            .iter()
            .map(|(id, state)| {
                JobChange::JobState(JobState {
                    job_id: JobId((*id).into()),
                    state: state.clone(),
                })
            })
            .chain(
                forgotten
                    .iter()
                    .map(|id| JobChange::Forgotten(JobId((*id).into()))),
            )
            .collect(),
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    let reopened = reopen(&directory);
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
        let memory = create(&directory, sequence.clone());
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    assert_eq!(
        observe(
            &memory,
            Input::Apply(Set::HistoryLength(HistoryLength(-1))),
            3
        ),
        Output::Applying(Applying::InvalidValue)
    );
    assert_eq!(memory.inspect().unwrap(), state);
    drop(memory);
    let reopened = reopen(&directory);
    assert_eq!(reopened.inspect().unwrap(), state);
}

#[test]
fn text_and_audio_expire_independently_and_only_matching_successful_deletions_are_forgotten() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
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
    let forgotten = execute(
        &memory,
        Input::Forget(vec![first.audio_location]),
        86401,
        None,
    );
    assert_eq!(forgotten.changes, changes(&[], &["first"]));
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    assert_eq!(result.changes, changes(&[], &["job"]));
    assert!(memory.inspect().unwrap().job_vector.is_empty());
}

#[test]
fn one_recording_stale_interrupt_and_refused_operations_leave_consistent_state() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
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

// Recover settles the delivery claim outstanding at the kill as Kept: its handoff may already
// have reached the desktop, so Memory never offers it again, and its text stays recallable.
// The recording job is normalized to Transcribing; the claimed job is the one change of
// state beyond that, and nothing else is offered.
#[test]
fn restart_returns_only_the_accepted_work_plan_and_normalizes_recording() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "transcribing", 0);
    stop(&memory, "transcribing");
    start(&memory, "waiting", 1);
    stop(&memory, "waiting");
    heard(&memory, "waiting", "ready");
    start(&memory, "recording", 2);
    drop(memory);
    let reopened = reopen(&directory);
    let result = execute(&reopened, Input::Recover, 3, None);
    let Output::Recovered(recovery) = result.output else {
        panic!()
    };
    assert_eq!(
        recovery.hear.0,
        vec![JobId("transcribing".into()), JobId("recording".into())]
    );
    assert_eq!(recovery.next, Next::Nothing);
    let record = reopened.inspect().unwrap();
    assert_eq!(record.delivery_claim.0, None);
    assert_eq!(record.job_vector[1].state, State::Delivered);
    assert_eq!(record.job_vector[1].handing_option, Some(Handing::Kept));
    assert_eq!(record.job_vector[2].state, State::Transcribing);
    assert_eq!(
        result.changes,
        changes(
            &[
                ("waiting", State::Delivered),
                ("recording", State::Transcribing),
            ],
            &[]
        )
    );
    assert_eq!(
        observe(&reopened, Input::Find(JobId("waiting".into())), 4),
        Output::Finding(Finding::Ready(ready("waiting", "ready", Delivery::Kept)))
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
    let memory = std::sync::Arc::new(create(&directory, Sequence::AsRecorded));
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
    let memory = create(&directory, Sequence::AsRecorded);
    let before = memory.inspect().unwrap();
    let mut invalid = before.configuration.recording_settings.clone();
    invalid.sample_rate = SampleRate(12345);
    assert_eq!(
        observe(&memory, Input::Apply(Set::RecordingSettings(invalid)), 0),
        Output::Applying(Applying::InvalidValue)
    );
    assert_eq!(memory.inspect().unwrap(), before);
    let mut invalid = before.configuration.recording_settings.clone();
    invalid.capture_fragment_bytes = CaptureFragmentBytes(3);
    assert_eq!(
        observe(&memory, Input::Apply(Set::RecordingSettings(invalid)), 0),
        Output::Applying(Applying::InvalidValue)
    );
    assert_eq!(memory.inspect().unwrap(), before);
    drop(memory);
    assert_eq!(reopen(&directory).inspect().unwrap(), before);
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

    // The ruled fold: a JobState inserts or replaces its job, and Delivered or Forgotten
    // removes it.
    impl Following for Jobs {
        type Change = JobChange;

        fn after(mut state: Self, change: JobChange) -> Self {
            match change {
                JobChange::JobState(job) => {
                    state.0.retain(|kept| kept.job_id != job.job_id);
                    if job.state != State::Delivered {
                        state.0.push(job);
                    }
                }
                JobChange::Forgotten(id) => state.0.retain(|kept| kept.job_id != id),
            }
            state
        }
    }
    struct Fixture {
        id: JobId,
        text: Text,
        jobs: Publisher<Jobs, JobChange>,
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
        ) -> Result<signal::Stream<Vec<JobState>, JobChange>, signal_listener::Output> {
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
    let memory = create(&directory, Sequence::AsRecorded);
    let committed = execute(
        &memory,
        listener_memory::operations::Input::Import(Import_Data {
            moment: Moment(17),
            text: fixture.text.clone(),
        }),
        17,
        Some(&fixture.id.0),
    );
    let [change] = &committed.changes.job_change_vector[..] else {
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
                meta_signal_listener::Refused_Data::UnknownBackend(UnknownBackend(BackendName(
                    "missing".into(),
                ))),
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
    let refusal: meta_signal_listener::Output = Potential::from("Refused.UnknownBackend.missing")
        .actualize(&mut budget())
        .unwrap();
    assert_eq!(
        refusal,
        meta_signal_listener::Output::Refused(meta_signal_listener::Refused_Data::UnknownBackend(
            UnknownBackend(BackendName("missing".into()))
        ))
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    let memory = create(&directory, Sequence::AsRecorded);
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
// own, RemoveBackend of an absent name refused (the ruling admits every RemoveBackend) or
// altering state, and backend definitions or Order lost on reopen.
#[test]
fn backend_settings_insert_replace_and_remove_by_name_and_survive_reopen() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
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
    let before = memory.inspect().unwrap();
    apply(&memory, Set::RemoveBackend(BackendName("spare".into())));
    assert_eq!(memory.inspect().unwrap(), before);
    apply(&memory, Set::Order(vec![whisper.clone(), speech.clone()]));
    let state = memory.inspect().unwrap();
    drop(memory);
    let reopened = reopen(&directory);
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    let reopened = reopen(&directory);
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    assert_eq!(
        execute(
            &memory,
            Input::Turn(Options {
                delivery_option: None,
                backend_name_vector_option: Some(vec![BackendName("missing".into())]),
            }),
            3,
            Some("unknown"),
        ),
        Committed {
            output: Output::Turning(Turning::UnknownBackend(UnknownBackend(BackendName(
                "missing".into()
            )))),
            changes: changes(&[], &[]),
        }
    );
    assert_eq!(memory.inspect().unwrap(), before);
    drop(memory);
    assert_eq!(reopen(&directory).inspect().unwrap(), before);
}

// RC13 coverage: AudioLocation archive identity. Catches a location rebuilt from the current
// AudioDirectory (at Prune or after reopen), an archive that drops or rewrites the directory,
// Prune marking audio erased itself, and Forget matching on the job identity alone.
#[test]
fn audio_location_archives_with_its_original_directory_across_reopen() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
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
    let memory = reopen(&directory);
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
        let memory = create(&directory, sequence.clone());
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
    let memory = create(&directory, Sequence::AsRecorded);
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
    assert_eq!(reopen(&directory).inspect().unwrap(), state);
}

// The Jobs stream as a subscriber holds it: the Opened snapshot, then each committed change in
// order. The ruled fold: a JobState inserts or replaces its job, a Delivered JobState removes
// it, and Forgotten removes it. The other foldings are deliberately wrong subscribers, kept
// beside the ruled one so the scenario below proves it reaches every removal path.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Folding {
    Ruled,
    KeepsDelivered,
    IgnoresForgotten,
    DropsFailedAndCancelled,
}
type JobView = std::collections::BTreeMap<String, State>;
fn fold(folding: Folding, mut jobs: JobView, change: &JobChange) -> JobView {
    match change {
        JobChange::JobState(job) => {
            let leaves = match folding {
                Folding::KeepsDelivered => false,
                Folding::DropsFailedAndCancelled => {
                    matches!(
                        job.state,
                        State::Delivered | State::Failed | State::Cancelled
                    )
                }
                Folding::Ruled | Folding::IgnoresForgotten => job.state == State::Delivered,
            };
            if leaves {
                jobs.remove(&job.job_id.0);
            } else {
                jobs.insert(job.job_id.0.clone(), job.state.clone());
            }
        }
        JobChange::Forgotten(id) => {
            if folding != Folding::IgnoresForgotten {
                jobs.remove(&id.0);
            }
        }
    }
    jobs
}
struct Subscriber {
    views: Vec<(Folding, JobView, bool)>,
}
impl Subscriber {
    fn open(memory: &listener_memory::Memory) -> Self {
        let opened = standing(memory);
        Self {
            views: [
                Folding::Ruled,
                Folding::KeepsDelivered,
                Folding::IgnoresForgotten,
                Folding::DropsFailedAndCancelled,
            ]
            .into_iter()
            .map(|folding| (folding, opened.clone(), false))
            .collect(),
        }
    }
    fn ruled(&self) -> &JobView {
        &self.views[0].1
    }
    // The ruled view equals a fresh Pending query; a wrong folding is recorded when it differs.
    fn agree(&mut self, memory: &listener_memory::Memory, after: &str) {
        let pending = standing(memory);
        for (folding, view, diverged) in &mut self.views {
            if *folding == Folding::Ruled {
                assert_eq!(
                    *view, pending,
                    "the ruled fold differs from Pending after {after}"
                );
            } else if *view != pending {
                *diverged = true;
            }
        }
    }
    fn follow(
        &mut self,
        memory: &listener_memory::Memory,
        input: Input,
        moment: i64,
        id: Option<&str>,
    ) -> Committed {
        let after = format!("{input:?}");
        let committed = execute(memory, input, moment, id);
        for (folding, view, _) in &mut self.views {
            for change in &committed.changes.job_change_vector {
                *view = fold(*folding, std::mem::take(view), change);
            }
        }
        self.agree(memory, &after);
        committed
    }
    // A refusal commits nothing, so nothing reaches the stream.
    fn refuse(&mut self, memory: &listener_memory::Memory, input: Input, moment: i64) -> Output {
        let after = format!("{input:?}");
        let output = observe(memory, input, moment);
        self.agree(memory, &after);
        output
    }
    // A failed operation returns no Committed, so it has no change to publish, and leaves the
    // store as it was.
    fn fail(
        &mut self,
        memory: &listener_memory::Memory,
        input: Input,
        moment: i64,
        id: Option<&str>,
    ) {
        let after = format!("{input:?}");
        let before = memory.inspect().unwrap();
        assert!(
            memory.execute(request(input, moment, id)).is_err(),
            "{after} was expected to fail"
        );
        assert_eq!(memory.inspect().unwrap(), before);
        self.agree(memory, &after);
    }
}
fn forgotten_ids(committed: &Committed) -> std::collections::BTreeSet<String> {
    committed
        .changes
        .job_change_vector
        .iter()
        .map(|change| match change {
            JobChange::Forgotten(id) => id.0.clone(),
            JobChange::JobState(job) => panic!("only Forgotten expected, found {job:?}"),
        })
        .collect()
}

// The Jobs stream's fold equals Pending through the whole ruled lifecycle: recording,
// transcribing (stop, Interrupt, Requeue, Recover after reopen), waiting, delivered, failed,
// cancelled, pruned and forgotten, with refused and failed operations interleaved. Catches a
// commit that moves a job without reporting it, a removal reported as anything but Forgotten
// (or not at all) by Prune or Forget, a Failed or Cancelled job dropped from Pending before it
// is Delivered or Forgotten, a change reported by a refused or failed operation, and an
// Opened snapshot that is not the Pending query. The wrong foldings must each diverge from
// Pending at least once, so the scenario cannot pass without reaching every removal path.
#[test]
fn the_ruled_fold_of_opened_and_committed_changes_equals_pending_through_the_lifecycle() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "early", 1);
    stop(&memory, "early");
    import(&memory, "old", 1, "old text");
    let mut subscriber = Subscriber::open(&memory);
    assert_eq!(
        *subscriber.ruled(),
        JobView::from([("early".to_string(), State::Transcribing)])
    );

    subscriber.follow(&memory, turn(), 2, Some("a"));
    assert_eq!(
        subscriber.refuse(&memory, Input::Requeue(JobId("a".into())), 2),
        Output::Requeueing(Requeueing::InProgress(JobState {
            job_id: JobId("a".into()),
            state: State::Recording,
        }))
    );
    subscriber.refuse(&memory, Input::Interrupt(JobId("missing".into())), 2);
    subscriber.fail(&memory, hearing("a", "too early"), 2, None);
    subscriber.fail(&memory, finishing("a"), 2, None);
    subscriber.follow(&memory, turn(), 3, None);
    let heard = subscriber.follow(&memory, hearing("a", "one"), 3, None);
    assert_eq!(
        heard.output,
        Output::Next(Next::Deliverable(ready("a", "one", Delivery::Kept)))
    );
    assert_eq!(subscriber.ruled().get("a"), Some(&State::Waiting));
    subscriber.fail(&memory, turn(), 3, Some("a"));
    subscriber.fail(&memory, turn(), 3, None);
    subscriber.fail(&memory, hearing("a", "twice"), 3, None);
    subscriber.follow(&memory, finishing("a"), 4, None);
    assert!(!subscriber.ruled().contains_key("a"));

    let failed = subscriber.follow(
        &memory,
        Input::Record(Record_Data {
            job_id: JobId("early".into()),
            hearing: Hearing::Exhausted(Vec::new()),
        }),
        5,
        None,
    );
    assert_eq!(failed.changes, changes(&[("early", State::Failed)], &[]));
    subscriber.follow(&memory, turn(), 6, Some("c"));
    subscriber.follow(&memory, Input::Drop, 6, None);
    subscriber.follow(&memory, turn(), 7, Some("i"));
    subscriber.follow(&memory, Input::Interrupt(JobId("i".into())), 7, None);
    subscriber.refuse(&memory, Input::Drop, 7);
    assert_eq!(subscriber.ruled().get("early"), Some(&State::Failed));
    assert_eq!(subscriber.ruled().get("c"), Some(&State::Cancelled));
    assert_eq!(subscriber.ruled().get("i"), Some(&State::Transcribing));

    // A delivered job requeued and exhausted keeps its earlier text: Failed, with text.
    capture(&memory, "f2", 8, "first text");
    subscriber.agree(&memory, "capture f2");
    subscriber.follow(&memory, Input::Requeue(JobId("f2".into())), 9, None);
    subscriber.follow(
        &memory,
        Input::Record(Record_Data {
            job_id: JobId("f2".into()),
            hearing: Hearing::Exhausted(Vec::new()),
        }),
        9,
        None,
    );
    assert_eq!(subscriber.ruled().get("f2"), Some(&State::Failed));

    // The Failed job stays in Pending until it is Delivered.
    subscriber.follow(&memory, Input::Requeue(JobId("early".into())), 10, None);
    subscriber.follow(&memory, hearing("early", "late"), 10, None);
    subscriber.follow(&memory, hearing("i", "interrupted"), 10, None);
    assert_eq!(subscriber.ruled().get("early"), Some(&State::Waiting));
    subscriber.follow(&memory, finishing("early"), 11, None);
    subscriber.follow(&memory, finishing("i"), 11, None);
    assert!(!subscriber.ruled().contains_key("early"));

    // Forget erases f2's audio; its text keeps it retained, Failed, in Pending.
    subscriber.follow(
        &memory,
        Input::Forget(vec![location("/fixture/audio-before", "f2")]),
        12,
        None,
    );
    assert_eq!(subscriber.ruled().get("f2"), Some(&State::Failed));
    subscriber.follow(
        &memory,
        Input::Apply(Set::TextKept(Retention::Days(Days(0)))),
        12,
        None,
    );
    let pruned = subscriber.follow(&memory, Input::Prune, 13, None);
    assert_eq!(
        forgotten_ids(&pruned),
        ["f2", "old"].map(String::from).into()
    );
    let record = memory.inspect().unwrap();
    for id in ["f2", "old"] {
        assert!(record.job_vector.iter().all(|job| job.job_id.0 != id));
    }
    assert_eq!(subscriber.ruled().get("c"), Some(&State::Cancelled));

    let forgotten = subscriber.follow(
        &memory,
        Input::Forget(vec![location("/fixture/audio-before", "c")]),
        14,
        None,
    );
    assert_eq!(forgotten.changes, changes(&[], &["c"]));
    assert!(!subscriber.ruled().contains_key("c"));
    assert!(
        memory
            .inspect()
            .unwrap()
            .job_vector
            .iter()
            .all(|job| job.job_id.0 != "c")
    );
    subscriber.refuse(
        &memory,
        Input::Forget(vec![location("/fixture/audio-before", "c")]),
        14,
    );

    subscriber.follow(&memory, turn(), 15, Some("restarted"));
    drop(memory);
    let memory = reopen(&directory);
    subscriber.agree(&memory, "reopen");
    subscriber.follow(&memory, Input::Recover, 16, None);
    assert_eq!(
        subscriber.ruled().get("restarted"),
        Some(&State::Transcribing)
    );
    for (folding, _, diverged) in &subscriber.views {
        assert!(
            *folding == Folding::Ruled || *diverged,
            "{folding:?} never diverged from Pending"
        );
    }
}

// RC13 coverage: AudioLocation Datom identity. Catches a Datom spelling that carries a second
// job identity beside the location, a location rebuilt from the current AudioDirectory, and a
// textual Forget that does not reach the job whose original directory it names.
#[cfg(feature = "datom")]
#[test]
fn datom_spells_an_audio_location_once_with_its_original_directory() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
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
    let memory = create(&directory, Sequence::AsRecorded);
    for (text, set) in cases {
        let decoded: meta_signal_listener::Input = decode(text).unwrap();
        assert_eq!(decoded, meta_signal_listener::Input::Configure(set.clone()));
        // The removal names a backend never configured; every RemoveBackend is admitted.
        apply(&memory, set);
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

// Fixtures for the repair scenarios (kvld). They build stores and inputs; none is a producer
// entry point.
fn name(text: &str) -> BackendName {
    BackendName(text.into())
}
fn definition_at(path: &std::path::Path) -> MemoryDefinition {
    MemoryDefinition {
        path: path.to_path_buf(),
        table_name: TABLE,
        record_key: KEY.into(),
    }
}
fn configured(sequence: Sequence, backends: &[&str], order: &[&str]) -> Configuration {
    Configuration {
        backend_vector: backends.iter().map(|text| backend(text, "model")).collect(),
        order: Order(order.iter().map(|text| name(text)).collect()),
        ..configuration(sequence)
    }
}
// A configuration with one path or recording Set applied, written independently of the
// producer's Set handling.
fn configured_with(mut configuration: Configuration, set: &Set) -> Configuration {
    match set.clone() {
        Set::PasteProgram(program) => configuration.paste_program = PasteProgram(program),
        Set::CopyProgram(program) => configuration.copy_program = CopyProgram(program),
        Set::SecretProgram(program) => configuration.secret_program = SecretProgram(program),
        Set::AudioDirectory(directory) => configuration.audio_directory = AudioDirectory(directory),
        Set::RecordingSettings(settings) => configuration.recording_settings = settings,
        other => panic!("no fixture for {other:?}"),
    }
    configuration
}
fn overriding(names: &[&str]) -> Input {
    Input::Turn(Options {
        delivery_option: None,
        backend_name_vector_option: Some(names.iter().map(|text| name(text)).collect()),
    })
}
fn trial(backends: Vec<Backend>) -> Output {
    Output::Trial(Trial_Data {
        backend_vector: backends,
        term_vector: Vec::new(),
    })
}
fn schema(version: u32) -> memory::Schema {
    memory::Schema {
        version: memory::SchemaVersion::new(version),
    }
}
// Writes a Memory record into a kernel store by hand, as a fixture for states the public
// operations cannot reach.
fn write_record(path: &std::path::Path, record: &listener_memory::state::Record) {
    let kernel = memory::Memory::open_with_schema(path, &schema(1)).unwrap();
    let table: memory::Table<&str, listener_memory::state::Record> = memory::Table::new(TABLE);
    kernel
        .write(|transaction| table.insert(transaction, KEY, record))
        .unwrap();
}
fn kernel_record(path: &std::path::Path) -> Option<listener_memory::state::Record> {
    let kernel = memory::Memory::open_with_schema(path, &schema(1)).unwrap();
    let table: memory::Table<&str, listener_memory::state::Record> = memory::Table::new(TABLE);
    kernel
        .read(|transaction| table.get(transaction, KEY))
        .unwrap()
}
fn standing(memory: &listener_memory::Memory) -> std::collections::BTreeMap<String, State> {
    let Output::Standing(jobs) = observe(memory, Input::Pending, 0) else {
        panic!("pending jobs expected")
    };
    let map: std::collections::BTreeMap<_, _> = jobs
        .iter()
        .map(|job| (job.job_id.0.clone(), job.state.clone()))
        .collect();
    assert_eq!(map.len(), jobs.len(), "a job listed twice: {jobs:?}");
    map
}

// The creation crash fixture. A child process runs one creator under a file-size limit, so
// the kernel kills it with SIGXFSZ, or refuses its write, wherever the store's file outgrows
// the limit; the limit doubles from 512 bytes until a creation completes. The kernel
// preallocates about a mebibyte when it first opens a file, so the fixture's configuration
// carries about three mebibytes of terms: the record's commit must then grow the file again,
// and one limit ends the child after the schema stamp and before the record commits. The
// "producer" creator is CreatingMemory::create; the "direct" creator is a deliberately wrong
// one that stamps the kernel schema at the final path and commits the record after it, as the
// pre-repair open did. A crash after the record commits and before publication grows no file
// and is out of this fixture's reach.
const CREATION_PATH: &str = "LISTENER_TESTS_CREATION_PATH";
const CREATOR: &str = "LISTENER_TESTS_CREATOR";
const SIGXFSZ: i32 = 25;
const REFUSED_WRITE: i32 = 70;
fn crash_configuration() -> Configuration {
    Configuration {
        term_vector: (0..48)
            .map(|index| Term(format!("{index:02}{}", "t".repeat(65_536))))
            .collect(),
        ..configuration(Sequence::AsReady)
    }
}
// What a kernel file holds, read without the Listener's open.
#[derive(Debug, PartialEq)]
enum Stage {
    Unstamped,
    StampedOnly,
    Recorded,
    Damaged,
}
fn stage(path: &std::path::Path) -> Stage {
    if std::fs::metadata(path).unwrap().len() == 0 {
        return Stage::Unstamped;
    }
    let Ok(kernel) = memory::Memory::open_with_schema(path, &schema(1)) else {
        return Stage::Damaged;
    };
    let table: memory::Table<&str, listener_memory::state::Record> = memory::Table::new(TABLE);
    match kernel.read(|transaction| table.get(transaction, KEY)) {
        Ok(Some(_)) => Stage::Recorded,
        Ok(None) => Stage::StampedOnly,
        Err(_) => Stage::Damaged,
    }
}
#[derive(Debug, PartialEq)]
enum FinalPath {
    Absent,
    Complete,
    Incomplete(Stage),
}
fn final_path(path: &std::path::Path) -> FinalPath {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FinalPath::Absent,
        Err(error) => panic!("final path unreadable: {error}"),
        Ok(_) => match stage(path) {
            Stage::Recorded => match definition_at(path).open() {
                Ok(memory) if settings(&memory) == crash_configuration() => FinalPath::Complete,
                _ => FinalPath::Incomplete(Stage::Recorded),
            },
            other => FinalPath::Incomplete(other),
        },
    }
}
#[derive(Debug, PartialEq)]
enum Ending {
    Completed,
    Killed,
    RefusedWrite,
}
#[derive(Debug)]
struct CreationRun {
    blocks: u64,
    ending: Ending,
    final_path: FinalPath,
    staged: Vec<Stage>,
    parent: TempDir,
}
fn creation_runs(creator: &str) -> Vec<CreationRun> {
    use std::os::unix::process::ExitStatusExt;
    let executable = std::env::current_exe().unwrap();
    let mut runs = Vec::new();
    for exponent in 0..=20 {
        let blocks = 1_u64 << exponent;
        let parent = TempDir::new().unwrap();
        let path = parent.path().join("state.memory");
        let status = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "ulimit -c 0; ulimit -f \"$1\" && exec \"$2\" --ignored --exact creation_child --test-threads 1",
                "sh",
            ])
            .arg(blocks.to_string())
            .arg(&executable)
            .env(CREATION_PATH, &path)
            .env(CREATOR, creator)
            .current_dir(parent.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        // Any other ending (a panic among them) is the fixture failing, not a crash window.
        let ending = match (status.code(), status.signal()) {
            (Some(0), _) => Ending::Completed,
            (Some(REFUSED_WRITE), _) => Ending::RefusedWrite,
            (None, Some(SIGXFSZ)) => Ending::Killed,
            _ => panic!("creation child ended otherwise at {blocks} blocks: {status}"),
        };
        let staged = std::fs::read_dir(parent.path())
            .unwrap()
            .map(|entry| entry.unwrap().path().join("state.memory"))
            .filter(|staged| staged.is_file())
            .map(|staged| stage(&staged))
            .collect();
        let completed = ending == Ending::Completed;
        runs.push(CreationRun {
            blocks,
            ending,
            final_path: final_path(&path),
            staged,
            parent,
        });
        if completed {
            return runs;
        }
    }
    panic!("no creation completed under any file-size limit: {runs:?}")
}
#[test]
#[ignore = "child process of the creation crash fixture; its parent names the creator"]
fn creation_child() {
    let path = std::path::PathBuf::from(std::env::var_os(CREATION_PATH).unwrap());
    let created = match std::env::var(CREATOR).unwrap().as_str() {
        "producer" => definition_at(&path).create(crash_configuration()).is_ok(),
        "direct" => {
            let kernel = memory::Memory::open_with_schema(&path, &schema(1)).unwrap();
            let table: memory::Table<&str, listener_memory::state::Record> =
                memory::Table::new(TABLE);
            let record = listener_memory::state::Record {
                job_vector: Vec::new(),
                configuration: crash_configuration(),
                delivery_claim: listener_memory::state::DeliveryClaim(None),
            };
            kernel
                .write(|transaction| table.insert(transaction, KEY, &record))
                .is_ok()
        }
        creator => panic!("unknown creator {creator}"),
    };
    std::process::exit(if created { 0 } else { REFUSED_WRITE });
}

// RC1: an abrupt end during first creation leaves no final file, whether it comes before the
// schema stamp or after it and before the record commits; a retry then creates a complete
// store. Catches a creation that stamps the kernel schema or commits the record at the final
// path (a crash leaves a stamped-only or empty file there, which every later open refuses),
// one that publishes before the record is durable, and one whose interrupted staging blocks
// a later creation.
#[test]
fn an_interrupted_creation_leaves_no_final_file_and_a_retry_creates_a_complete_store() {
    let runs = creation_runs("producer");
    for window in [Stage::Unstamped, Stage::StampedOnly] {
        assert!(
            runs.iter().any(|run| run.ending != Ending::Completed
                && matches!(run.staged.as_slice(), [only] if *only == window)),
            "the fixture did not interrupt staging at this window: {runs:?}"
        );
    }
    for run in &runs {
        let path = run.parent.path().join("state.memory");
        if run.ending == Ending::Completed {
            assert_eq!(run.final_path, FinalPath::Complete, "{run:?}");
            continue;
        }
        assert_eq!(run.final_path, FinalPath::Absent, "{run:?}");
        assert!(
            matches!(definition_at(&path).open(), Err(Error::MissingStore)),
            "after the run at {} blocks",
            run.blocks
        );
        assert_eq!(final_path(&path), FinalPath::Absent);
        let retried = definition_at(&path)
            .create(configuration(Sequence::AsRecorded))
            .unwrap();
        start(&retried, "after", 1);
        let state = retried.inspect().unwrap();
        drop(retried);
        let reopened = definition_at(&path).open().unwrap();
        assert_eq!(reopened.inspect().unwrap(), state);
        assert_eq!(settings(&reopened), configuration(Sequence::AsRecorded));
    }
}

// The crash fixture's own discrimination: the same limits applied to the deliberately wrong
// direct creator leave an empty and a stamped-only final file (the pre-repair failure shape),
// so the fixture reaches the windows that matter and the test above cannot pass vacuously.
#[test]
fn the_creation_crash_fixture_catches_a_creator_that_writes_the_final_path() {
    let runs = creation_runs("direct");
    for window in [Stage::Unstamped, Stage::StampedOnly] {
        let shape = FinalPath::Incomplete(window);
        assert!(
            runs.iter()
                .any(|run| run.ending != Ending::Completed && run.final_path == shape),
            "{runs:?}"
        );
    }
}

// RC1: a stamped-only final file (the pre-repair failure shape) is refused and never
// populated. Catches an open that initializes a missing record under the persisted path
// (resuming creation with a supplied or default configuration), a create that treats the
// existing file as its own, a schema-mismatched or foreign file that is reinitialized, and a
// directory at the path treated as a store.
#[test]
fn a_stamped_only_mismatched_or_foreign_final_path_is_refused_and_left_as_it_was() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("stamped.memory");
    drop(memory::Memory::open_with_schema(&path, &schema(1)).unwrap());
    for _ in 0..2 {
        assert!(matches!(
            definition_at(&path).open(),
            Err(Error::MissingRecord)
        ));
        assert!(matches!(
            definition_at(&path).create(configuration(Sequence::AsReady)),
            Err(Error::StoreAlreadyExists)
        ));
        assert_eq!(kernel_record(&path), None);
    }

    let mismatched = directory.path().join("mismatched.memory");
    drop(memory::Memory::open_with_schema(&mismatched, &schema(2)).unwrap());
    assert!(definition_at(&mismatched).open().is_err());
    assert!(matches!(
        definition_at(&mismatched).create(configuration(Sequence::AsReady)),
        Err(Error::StoreAlreadyExists)
    ));
    assert!(memory::Memory::open_with_schema(&mismatched, &schema(2)).is_ok());

    let foreign = directory.path().join("foreign.memory");
    std::fs::write(&foreign, b"not a store\n").unwrap();
    assert!(definition_at(&foreign).open().is_err());
    assert!(matches!(
        definition_at(&foreign).create(configuration(Sequence::AsReady)),
        Err(Error::StoreAlreadyExists)
    ));
    assert_eq!(std::fs::read(&foreign).unwrap(), b"not a store\n");

    let folder = directory.path().join("folder.memory");
    std::fs::create_dir(&folder).unwrap();
    assert!(matches!(
        definition_at(&folder).open(),
        Err(Error::StoreNotRegular)
    ));
    assert!(matches!(
        definition_at(&folder).create(configuration(Sequence::AsReady)),
        Err(Error::StoreAlreadyExists)
    ));
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
}

// RC1: competing creation cannot replace the winner. Catches a check-then-rename creation
// (rename replaces the winner, so two creators succeed or the store holds a loser's
// configuration), a create that opens an existing store as if it had created it, and a
// loser that alters the winner's record.
#[test]
fn concurrent_creations_publish_exactly_one_store_and_never_replace_it() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("raced.memory");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|index| {
            let barrier = barrier.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                let configuration = Configuration {
                    history_length: HistoryLength(index),
                    ..configuration(Sequence::AsReady)
                };
                barrier.wait();
                (index, definition_at(&path).create(configuration))
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    let mut winners = Vec::new();
    for (index, result) in &results {
        match result {
            Ok(_) => winners.push(*index),
            Err(Error::StoreAlreadyExists) => {}
            Err(error) => panic!("creator {index} failed otherwise: {error}"),
        }
    }
    assert_eq!(winners.len(), 1, "creators that succeeded: {winners:?}");
    let winner = winners[0];
    drop(results);
    let memory = definition_at(&path).open().unwrap();
    assert_eq!(settings(&memory).history_length, HistoryLength(winner));
    import(&memory, "kept", 1, "kept text");
    let state = memory.inspect().unwrap();
    drop(memory);
    assert!(matches!(
        definition_at(&path).create(configuration(Sequence::AsRecorded)),
        Err(Error::StoreAlreadyExists)
    ));
    assert_eq!(
        definition_at(&path).open().unwrap().inspect().unwrap(),
        state
    );
}

// RC5: create and open are distinct. Catches an open of a missing path that creates the
// database or its parent directories (the pre-repair open created), a create that validates
// after making the parent or staging (a refused configuration leaves debris), and a create
// over an existing store that changes it.
#[test]
fn open_of_a_missing_path_makes_nothing_and_a_refused_create_leaves_nothing() {
    let directory = TempDir::new().unwrap();
    let nested = directory.path().join("absent").join("deeper");
    let path = nested.join("state.memory");
    assert!(matches!(
        definition_at(&path).open(),
        Err(Error::MissingStore)
    ));
    assert!(matches!(
        definition(&directory).open(),
        Err(Error::MissingStore)
    ));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    let unknown = configured(Sequence::AsReady, &["speech"], &["speech", "missing"]);
    assert!(matches!(
        definition_at(&path).create(unknown),
        Err(Error::UnknownBackend(BackendName(text))) if text == "missing"
    ));
    let invalid = Configuration {
        kilobits: Kilobits(0),
        ..configuration(Sequence::AsReady)
    };
    assert!(matches!(
        definition_at(&path).create(invalid),
        Err(Error::InvalidConfiguration)
    ));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    let memory = definition_at(&path)
        .create(configuration(Sequence::AsReady))
        .unwrap();
    capture(&memory, "kept", 1, "kept text");
    let state = memory.inspect().unwrap();
    drop(memory);
    assert!(matches!(
        definition_at(&path).create(configured(Sequence::AsRecorded, &[], &[])),
        Err(Error::StoreAlreadyExists)
    ));
    assert_eq!(
        definition_at(&path).open().unwrap().inspect().unwrap(),
        state
    );
}

// RC5: the persisted configuration is authoritative on open. Each policy is created in its
// own store and reopened without a configuration; Recover then answers by the stored policy.
// Catches an open that substitutes a default configuration of either policy (one of the two
// stores then answers the other's way) and a create whose configuration is not the one that
// persists. Under AsReady the claim "second" held at the kill is settled Kept, not replayed;
// under AsRecorded no claim existed, and "second" waits behind the Transcribing "first".
#[test]
fn reopen_recovers_under_the_persisted_policy_of_each_separately_created_store() {
    for (sequence, second) in [
        (Sequence::AsReady, State::Delivered),
        (Sequence::AsRecorded, State::Waiting),
    ] {
        let directory = TempDir::new().unwrap();
        let memory = create(&directory, sequence.clone());
        start(&memory, "first", 1);
        stop(&memory, "first");
        start(&memory, "second", 2);
        stop(&memory, "second");
        heard(&memory, "second", "two");
        drop(memory);
        let memory = reopen(&directory);
        assert_eq!(settings(&memory).sequence, sequence);
        let result = execute(&memory, Input::Recover, 3, None);
        assert_eq!(
            result.output,
            Output::Recovered(Recovery {
                hear: Hear(vec![JobId("first".into())]),
                next: Next::Nothing,
            })
        );
        let record = memory.inspect().unwrap();
        assert_eq!(record.delivery_claim.0, None);
        assert_eq!(job(&record, "second").state, second);
        let settled: Vec<(&str, State)> = if second == State::Delivered {
            vec![("second", State::Delivered)]
        } else {
            Vec::new()
        };
        assert_eq!(result.changes, changes(&settled, &[]));
    }
}

// An outstanding claim is settled on every open, not only the first: a store reopened and
// recovered again, or probed by an explicit advance, offers the settled job nothing further.
// Catches a Recover that re-arms the claim it settled, an Advance that offers it, and a
// Recover on a settled store that reports a change.
#[test]
fn an_outstanding_claim_is_settled_kept_and_never_replayed_across_repeated_restarts() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "claimed", 1);
    stop(&memory, "claimed");
    assert_eq!(ready_id(heard(&memory, "claimed", "one")), "claimed");
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(
        changed(
            &memory,
            Input::Recover,
            2,
            None,
            changes(&[("claimed", State::Delivered)], &[]),
        ),
        Output::Recovered(Recovery {
            hear: Hear(Vec::new()),
            next: Next::Nothing,
        })
    );
    assert_eq!(
        job(&memory.inspect().unwrap(), "claimed").handing_option,
        Some(Handing::Kept)
    );
    drop(memory);
    for moment in 3..6 {
        let memory = reopen(&directory);
        assert_eq!(
            changed(&memory, Input::Recover, moment, None, changes(&[], &[])),
            Output::Recovered(Recovery {
                hear: Hear(Vec::new()),
                next: Next::Nothing,
            })
        );
        assert_eq!(
            observe(&memory, Input::Advance, moment),
            Output::Next(Next::Nothing)
        );
        assert_eq!(memory.inspect().unwrap().delivery_claim.0, None);
        drop(memory);
    }
}

// The Waiting job behind the claim drains once the claim is settled: the first Next after the
// settlement offers it, exactly once, and finishing it leaves nothing. Catches a settlement that
// leaves the claim armed (the later job is never offered), an offer made twice, and a later job
// that is offered while the claim it waits behind is still outstanding.
#[test]
fn later_waiting_jobs_drain_once_the_outstanding_claim_is_settled() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "claimed", 1);
    stop(&memory, "claimed");
    assert_eq!(ready_id(heard(&memory, "claimed", "one")), "claimed");
    start(&memory, "later", 2);
    stop(&memory, "later");
    assert_eq!(heard(&memory, "later", "two"), Next::Nothing);
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(
        changed(
            &memory,
            Input::Recover,
            3,
            None,
            changes(&[("claimed", State::Delivered)], &[]),
        ),
        Output::Recovered(Recovery {
            hear: Hear(Vec::new()),
            next: Next::Deliverable(ready("later", "two", Delivery::Kept)),
        })
    );
    assert_eq!(finish(&memory, "later"), Next::Nothing);
    assert_eq!(
        observe(&memory, Input::Advance, 4),
        Output::Next(Next::Nothing)
    );
}

// An explicit Recall or RecallJob is answered once with the settled text and records nothing:
// Memory's Last and Find are reads, so the Nexus performs one handoff per request, and no claim
// is armed, so no handoff follows on its own. Catches a read that arms a claim, a Find or Last
// that answers with nothing for a settled job, and an answer that changes the store.
#[test]
fn explicit_recall_answers_the_settled_text_once_and_starts_no_automatic_handoff() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "claimed", 1);
    stop(&memory, "claimed");
    assert_eq!(ready_id(heard(&memory, "claimed", "one")), "claimed");
    drop(memory);
    let memory = reopen(&directory);
    execute(&memory, Input::Recover, 2, None);
    assert_eq!(
        observe(&memory, Input::Last, 3),
        Output::Lasting(Lasting::Ready(ready("claimed", "one", Delivery::Kept)))
    );
    assert_eq!(
        observe(&memory, Input::Find(JobId("claimed".into())), 3),
        Output::Finding(Finding::Ready(ready("claimed", "one", Delivery::Kept)))
    );
    assert_eq!(
        observe(&memory, Input::Advance, 3),
        Output::Next(Next::Nothing)
    );
    assert_eq!(memory.inspect().unwrap().delivery_claim.0, None);
    assert_eq!(
        job(&memory.inspect().unwrap(), "claimed").state,
        State::Delivered
    );
}

// The claim invariant behind Recover's settlement: while a claim is outstanding, every input the
// Nexus can send leaves the claimed job Waiting and the claim in place, so the Kept settlement
// cannot find its job in another state. Each refusal is a refusal, not a change. Catches an
// input that moves the claimed job or clears the claim, and a Recover that would fail on a claim
// the public inputs can build.
#[test]
fn a_claimed_job_stays_waiting_until_it_is_finished_or_recovered() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "claimed", 1);
    stop(&memory, "claimed");
    assert_eq!(ready_id(heard(&memory, "claimed", "one")), "claimed");
    let holds_claim = |memory: &listener_memory::Memory| {
        let record = memory.inspect().unwrap();
        assert_eq!(record.delivery_claim.0, Some(JobId("claimed".into())));
        assert_eq!(job(&record, "claimed").state, State::Waiting);
    };
    holds_claim(&memory);
    execute(&memory, turn(), 2, Some("fresh"));
    holds_claim(&memory);
    execute(&memory, Input::Interrupt(JobId("fresh".into())), 2, None);
    holds_claim(&memory);
    execute(&memory, hearing("fresh", "later"), 2, None);
    holds_claim(&memory);
    assert!(
        memory
            .execute(request(hearing("claimed", "again"), 2, None))
            .is_err()
    );
    holds_claim(&memory);
    assert!(
        memory
            .execute(request(finishing("fresh"), 2, None))
            .is_err()
    );
    holds_claim(&memory);
    assert!(
        memory
            .execute(request(Input::AbortStart(JobId("fresh".into())), 2, None))
            .is_err()
    );
    holds_claim(&memory);
    for input in [
        Input::Requeue(JobId("claimed".into())),
        Input::Requeue(JobId("fresh".into())),
        Input::Tries(JobId("claimed".into())),
        Input::Drop,
        Input::Prune,
        Input::Forget(Vec::new()),
        Input::Last,
        Input::Find(JobId("claimed".into())),
        Input::Pending,
        Input::Recollect,
        Input::Settings,
        Input::Advance,
        Input::Apply(Set::AddTerm(term("spelling"))),
    ] {
        execute(&memory, input, 3, None);
        holds_claim(&memory);
    }
    import(&memory, "imported", 4, "text");
    holds_claim(&memory);
    assert_eq!(
        execute(&memory, Input::Recover, 5, None).changes,
        changes(&[("claimed", State::Delivered)], &[])
    );
    assert_eq!(
        memory.inspect().unwrap().delivery_claim.0,
        Some(JobId("fresh".into()))
    );
}

// RC2: removing a selected backend while its job records. Tries keeps the job's selected
// names that are still configured, in their order, with their current definitions. Catches
// a removal refused while a job names the backend, Tries failing on the removed name (the
// pre-repair UnknownBackend), Tries still offering the removed backend, Tries falling back
// to the configured Order while a selected backend remains, a snapshot rewritten by the
// removal, and an Order that keeps the removed name.
#[test]
fn removing_a_selected_backend_while_recording_keeps_the_remaining_selection_in_order() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .create(configured(
            Sequence::AsRecorded,
            &["speech", "whisper", "spare"],
            &["spare", "speech"],
        ))
        .unwrap();
    changed(
        &memory,
        overriding(&["speech", "whisper"]),
        1,
        Some("job"),
        changes(&[("job", State::Recording)], &[]),
    );
    apply(&memory, Set::RemoveBackend(name("speech")));
    let configuration = settings(&memory);
    assert_eq!(
        configuration.backend_vector,
        vec![backend("whisper", "model"), backend("spare", "model")]
    );
    assert_eq!(configuration.order, Order(vec![name("spare")]));
    stop(&memory, "job");
    assert_eq!(
        job(&memory.inspect().unwrap(), "job").backend_name_vector,
        vec![name("speech"), name("whisper")]
    );
    assert_eq!(
        observe(&memory, Input::Tries(JobId("job".into())), 2),
        trial(vec![backend("whisper", "model")])
    );
    apply(&memory, Set::Backend(backend("whisper", "whisper-two")));
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(
        observe(&memory, Input::Tries(JobId("job".into())), 3),
        trial(vec![backend("whisper", "whisper-two")])
    );
}

// RC2: removing every selected backend while the job transcribes, then every backend. Tries
// falls back to the configured Order, and with neither left offers nothing, so the caller
// exhausts the job. Catches a job wedged by unresolvable names (Recover skipping it or Tries
// failing), a fallback that ignores the configured Order, an empty Trial while the Order has
// backends, a removal of an absent name refused, an exhausted job still blocking later
// text, a retained terminal job that cannot be requeued after removal, and a requeued job
// that does not take the post-removal Order.
#[test]
fn removing_every_backend_while_transcribing_falls_back_then_exhausts_without_wedging() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .create(configured(
            Sequence::AsRecorded,
            &["speech", "whisper", "spare"],
            &["spare"],
        ))
        .unwrap();
    changed(
        &memory,
        overriding(&["speech", "whisper"]),
        1,
        Some("job"),
        changes(&[("job", State::Recording)], &[]),
    );
    stop(&memory, "job");
    start(&memory, "later", 2);
    stop(&memory, "later");
    assert_eq!(heard(&memory, "later", "later text"), Next::Nothing);
    apply(&memory, Set::RemoveBackend(name("speech")));
    apply(&memory, Set::RemoveBackend(name("whisper")));
    assert_eq!(
        observe(&memory, Input::Tries(JobId("job".into())), 3),
        trial(vec![backend("spare", "model")])
    );
    let before = memory.inspect().unwrap();
    apply(&memory, Set::RemoveBackend(name("speech")));
    assert_eq!(memory.inspect().unwrap(), before);
    apply(&memory, Set::RemoveBackend(name("spare")));
    let configuration = settings(&memory);
    assert!(configuration.backend_vector.is_empty());
    assert_eq!(configuration.order, Order(Vec::new()));
    for id in ["job", "later"] {
        assert_eq!(
            observe(&memory, Input::Tries(JobId(id.into())), 4),
            trial(Vec::new())
        );
    }
    assert_eq!(
        observe(&memory, turn(), 4),
        Output::Turning(Turning::NoBackend)
    );
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(
        execute(&memory, Input::Recover, 5, None).output,
        Output::Recovered(Recovery {
            hear: Hear(vec![JobId("job".into())]),
            next: Next::Nothing,
        })
    );
    assert_eq!(
        observe(&memory, Input::Tries(JobId("job".into())), 5),
        trial(Vec::new())
    );
    let exhausted = exhaust(&memory, "job", Vec::new());
    assert_eq!(
        exhausted,
        Committed {
            output: Output::Next(Next::Deliverable(ready(
                "later",
                "later text",
                Delivery::Kept
            ))),
            changes: changes(&[("job", State::Failed)], &[]),
        }
    );
    assert_eq!(finish(&memory, "later"), Next::Nothing);
    changed(
        &memory,
        Input::Requeue(JobId("job".into())),
        6,
        None,
        changes(&[("job", State::Transcribing)], &[]),
    );
    apply(&memory, Set::Backend(backend("fresh", "fresh-one")));
    apply(&memory, Set::Order(vec![name("fresh")]));
    assert_eq!(
        observe(&memory, Input::Tries(JobId("job".into())), 7),
        trial(vec![backend("fresh", "fresh-one")])
    );
}

// RC3: invalid configuration values are refused as the meta InvalidValue unit, distinct from
// UnknownBackend, with no committed change and state unchanged after reopen; each valid Set
// commits exactly one Set. Catches a refusal surfaced as a Memory error (the pre-repair
// shape), a range check off by one at an accepted boundary, a refusal that still writes or
// reports the Set, an UnknownBackend reported as InvalidValue or the reverse, and a valid Set
// that is lost on reopen.
#[test]
fn invalid_configuration_values_are_refused_as_invalid_value_with_no_committed_change() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
    let base = configuration(Sequence::AsRecorded).recording_settings;
    let recording = |change: fn(&mut RecordingSettings)| {
        let mut settings = base.clone();
        change(&mut settings);
        Set::RecordingSettings(settings)
    };
    let budget = |steps: i64, depth: i64| {
        Set::Budget(translator::DatomReadingBudget {
            reading_steps: translator::ReadingSteps(steps),
            reading_depth: translator::ReadingDepth(depth),
        })
    };
    let timed = |seconds: i64| {
        Set::Backend(Backend {
            timeout: Timeout(Seconds(seconds)),
            ..backend("speech", "model")
        })
    };
    let invalid = [
        Set::HistoryLength(HistoryLength(-1)),
        Set::Kilobits(Kilobits(0)),
        Set::Kilobits(Kilobits(-24)),
        Set::AudioKept(Retention::Days(Days(-1))),
        Set::TextKept(Retention::Days(Days(-1))),
        timed(0),
        timed(-30),
        budget(0, 64),
        budget(1024, 0),
        budget(-1, 64),
        recording(|settings| settings.sample_rate = SampleRate(12345)),
        recording(|settings| settings.capture_fragment_bytes = CaptureFragmentBytes(0)),
        recording(|settings| settings.capture_fragment_bytes = CaptureFragmentBytes(3)),
        recording(|settings| settings.capture_buffer_bytes = CaptureBufferBytes(3836)),
        recording(|settings| settings.capture_buffer_bytes = CaptureBufferBytes(15362)),
        recording(|settings| settings.pending_pcm_frames = PendingPcmFrames(0)),
        recording(|settings| settings.page_packets = PagePackets(0)),
        recording(|settings| settings.encoding_complexity = EncodingComplexity(11)),
        recording(|settings| settings.encoding_complexity = EncodingComplexity(-1)),
        recording(|settings| settings.expected_packet_loss = ExpectedPacketLoss(101)),
        recording(|settings| settings.expected_packet_loss = ExpectedPacketLoss(-1)),
    ];
    let before = memory.inspect().unwrap();
    for set in invalid {
        assert_eq!(
            observe(&memory, Input::Apply(set.clone()), 1),
            Output::Applying(Applying::InvalidValue),
            "{set:?}"
        );
    }
    for order in [vec![name("missing")], vec![name("speech"), name("missing")]] {
        assert_eq!(
            observe(&memory, Input::Apply(Set::Order(order)), 1),
            Output::Applying(Applying::UnknownBackend(UnknownBackend(name("missing"))))
        );
    }
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(memory.inspect().unwrap(), before);
    let valid = [
        Set::HistoryLength(HistoryLength(0)),
        Set::Kilobits(Kilobits(1)),
        Set::AudioKept(Retention::Days(Days(0))),
        Set::TextKept(Retention::Days(Days(0))),
        timed(1),
        budget(1, 1),
        recording(|settings| settings.encoding_complexity = EncodingComplexity(10)),
        recording(|settings| settings.expected_packet_loss = ExpectedPacketLoss(100)),
        Set::PasteProgram(Program("/fixture/paste-two".into())),
        Set::CopyProgram(Program("/fixture/copy-two".into())),
        Set::SecretProgram(Program("/fixture/secret-two".into())),
        Set::AudioDirectory(Directory("/fixture/audio-two".into())),
    ];
    for set in valid {
        apply(&memory, set);
    }
    let state = memory.inspect().unwrap();
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(memory.inspect().unwrap(), state);
    let configuration = settings(&memory);
    assert_eq!(configuration.history_length, HistoryLength(0));
    assert_eq!(
        configuration.budget,
        Budget(translator::DatomReadingBudget {
            reading_steps: translator::ReadingSteps(1),
            reading_depth: translator::ReadingDepth(1),
        })
    );
    assert_eq!(
        configuration.paste_program,
        PasteProgram(Program("/fixture/paste-two".into()))
    );
    assert_eq!(
        configuration.copy_program,
        CopyProgram(Program("/fixture/copy-two".into()))
    );
    assert_eq!(
        configuration.secret_program,
        SecretProgram(Program("/fixture/secret-two".into()))
    );
    assert_eq!(
        configuration.audio_directory,
        AudioDirectory(Directory("/fixture/audio-two".into()))
    );
}

// RC6: configured paths are lexically absolute and nonempty without NUL; Pulse names are
// nonempty without NUL; nothing else is required of either. Each refused value is refused on
// create (nothing made) and atomically on Set. Catches a relative or empty path accepted, a
// NUL accepted, a path canonicalized or probed on the host (a nonexistent or non-canonical
// absolute path refused, or stored rewritten), and an invented Pulse syntax that refuses a
// nonempty name.
#[test]
fn paths_must_be_absolute_and_pulse_names_nonempty_without_nul_on_create_and_set() {
    let accepted_paths = [
        "/",
        "/nonexistent/listener/fixture",
        "/fixture/a/../b",
        "/fixture//double",
        "/fixture/with space",
    ];
    let refused_paths = [
        "", "relative", "./paste", "../paste", "~/paste", "/pa\0ste", "\0",
    ];
    let path_sets: [fn(&str) -> Set; 4] = [
        |text| Set::PasteProgram(Program(text.into())),
        |text| Set::CopyProgram(Program(text.into())),
        |text| Set::SecretProgram(Program(text.into())),
        |text| Set::AudioDirectory(Directory(text.into())),
    ];
    let accepted_names = ["listener", "Listener Fixture", "not:a//pulse name", "x"];
    let refused_names = ["", "list\0ener"];
    let base = configuration(Sequence::AsReady).recording_settings;
    let named = |text: &str| {
        Set::RecordingSettings(RecordingSettings {
            pulse_application_name: PulseApplicationName(text.into()),
            ..base.clone()
        })
    };
    let served = |text: &str| {
        Set::RecordingSettings(RecordingSettings {
            pulse_server: PulseServer::Named(AudioServerAddress(text.into())),
            ..base.clone()
        })
    };
    let mut refused: Vec<Set> = Vec::new();
    for set in path_sets {
        refused.extend(refused_paths.iter().map(|text| set(text)));
    }
    refused.extend(refused_names.iter().map(|text| named(text)));
    refused.extend(refused_names.iter().map(|text| served(text)));
    let mut accepted: Vec<Set> = Vec::new();
    for set in path_sets {
        accepted.extend(accepted_paths.iter().map(|text| set(text)));
    }
    accepted.extend(accepted_names.iter().map(|text| named(text)));
    accepted.extend(
        [
            "unix:/run/user/1000/pulse/native",
            "tcp:localhost:4713",
            "localhost",
        ]
        .iter()
        .map(|text| served(text)),
    );

    let directory = TempDir::new().unwrap();
    let refused_create = directory.path().join("refused").join("state.memory");
    for set in &refused {
        assert!(
            matches!(
                definition_at(&refused_create)
                    .create(configured_with(configuration(Sequence::AsReady), set)),
                Err(Error::InvalidConfiguration)
            ),
            "{set:?}"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
    let mut memory = create(&directory, Sequence::AsReady);
    capture(&memory, "kept", 1, "kept text");
    let before = memory.inspect().unwrap();
    for set in &refused {
        assert_eq!(
            observe(&memory, Input::Apply(set.clone()), 2),
            Output::Applying(Applying::InvalidValue),
            "{set:?}"
        );
    }
    drop(memory);
    memory = reopen(&directory);
    assert_eq!(memory.inspect().unwrap(), before);
    for set in accepted {
        let expected = configured_with(settings(&memory), &set);
        apply(&memory, set.clone());
        drop(memory);
        memory = reopen(&directory);
        assert_eq!(settings(&memory), expected, "{set:?}");
        let accepted_create = directory.path().join("accepted").join("state.memory");
        let created = definition_at(&accepted_create)
            .create(configured_with(configuration(Sequence::AsReady), &set))
            .unwrap();
        assert_eq!(
            settings(&created),
            configured_with(configuration(Sequence::AsReady), &set)
        );
        drop(created);
        std::fs::remove_dir_all(directory.path().join("accepted")).unwrap();
    }
    assert_eq!(
        job(&memory.inspect().unwrap(), "kept").audio,
        Audio::Kept(Directory("/fixture/audio-before".into()))
    );
}

// RC4: an unknown override backend is refused as the shared UnknownBackend naming the first
// unknown name, distinct from NoBackend, with nothing recorded. Catches an override that
// starts a recording and fails later, a refusal that names the wrong backend or commits a
// job (the refused identity then cannot start a recording), an empty override treated as
// unknown or as the configured Order, and an override checked against the Order instead of
// the configured backends.
#[test]
fn an_unknown_override_backend_is_refused_by_name_and_records_nothing() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .create(configured(
            Sequence::AsReady,
            &["speech", "whisper"],
            &["speech"],
        ))
        .unwrap();
    let before = memory.inspect().unwrap();
    for names in [&["missing"][..], &["speech", "missing", "absent"][..]] {
        assert_eq!(
            execute(&memory, overriding(names), 1, Some("job")),
            Committed {
                output: Output::Turning(Turning::UnknownBackend(UnknownBackend(name("missing")))),
                changes: changes(&[], &[]),
            }
        );
    }
    assert_eq!(
        execute(&memory, overriding(&[]), 1, Some("job")),
        Committed {
            output: Output::Turning(Turning::NoBackend),
            changes: changes(&[], &[]),
        }
    );
    assert_eq!(memory.inspect().unwrap(), before);
    changed(
        &memory,
        overriding(&["whisper"]),
        1,
        Some("job"),
        changes(&[("job", State::Recording)], &[]),
    );
    assert_eq!(
        job(&memory.inspect().unwrap(), "job").backend_name_vector,
        vec![name("whisper")]
    );
}

// RC4: Requeue of a job still in progress is refused as InProgress carrying its JobState;
// terminal jobs keep their own answers. Catches a Requeue that resets a recording,
// transcribing or waiting job (losing its delivery claim or tries), an InProgress that carries
// a wrong or fixed state, a nonterminal job answered NoAudio or Requeued, a terminal job
// answered InProgress, and the pre-repair order that tested audio before state (see the
// crafted case below).
#[test]
fn requeue_refuses_in_progress_jobs_with_their_state_and_answers_terminal_jobs_by_audio() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    capture(&memory, "delivered", 1, "spoken");
    import(&memory, "imported", 2, "remembered");
    start(&memory, "failed", 3);
    stop(&memory, "failed");
    exhaust(&memory, "failed", Vec::new());
    start(&memory, "transcribing", 4);
    stop(&memory, "transcribing");
    start(&memory, "waiting", 5);
    stop(&memory, "waiting");
    assert_eq!(ready_id(heard(&memory, "waiting", "ready")), "waiting");
    start(&memory, "recording", 6);
    for (id, state) in [
        ("recording", State::Recording),
        ("transcribing", State::Transcribing),
        ("waiting", State::Waiting),
    ] {
        assert_eq!(
            observe(&memory, Input::Requeue(JobId(id.into())), 7),
            Output::Requeueing(Requeueing::InProgress(JobState {
                job_id: JobId(id.into()),
                state,
            }))
        );
    }
    assert_eq!(
        observe(&memory, Input::Requeue(JobId("imported".into())), 7),
        Output::Requeueing(Requeueing::NoAudio(signal_listener::NoAudio(JobId(
            "imported".into()
        ))))
    );
    assert_eq!(
        observe(&memory, Input::Requeue(JobId("missing".into())), 7),
        Output::Requeueing(Requeueing::NoSuchJob(signal_listener::NoSuchJob(JobId(
            "missing".into()
        ))))
    );
    for id in ["delivered", "failed"] {
        assert_eq!(
            changed(
                &memory,
                Input::Requeue(JobId(id.into())),
                7,
                None,
                changes(&[(id, State::Transcribing)], &[]),
            ),
            Output::Requeueing(Requeueing::Requeued(JobId(id.into())))
        );
    }
    assert_eq!(finish(&memory, "waiting"), Next::Nothing);
}

// RC4 priority: a job both in progress and without audio answers InProgress. No public
// operation reaches that state, so the record is written into the kernel store by hand and
// opened. Catches the pre-repair order, which answered NoAudio before testing state.
#[test]
fn in_progress_takes_priority_over_missing_audio_for_requeue() {
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    start(&memory, "erased", 1);
    stop(&memory, "erased");
    let path = memory.path().unwrap();
    let mut record = memory.inspect().unwrap();
    drop(memory);
    record.job_vector[0].audio = Audio::Erased;
    write_record(&path, &record);
    let memory = reopen(&directory);
    assert_eq!(memory.inspect().unwrap(), record);
    assert_eq!(
        observe(&memory, Input::Requeue(JobId("erased".into())), 2),
        Output::Requeueing(Requeueing::InProgress(JobState {
            job_id: JobId("erased".into()),
            state: State::Transcribing,
        }))
    );
}

// RC3/RC4 contract shapes: the ordinary Refused carries the shared UnknownBackend and
// InProgress(JobState); the meta Refused carries the same shared UnknownBackend and the unit
// InvalidValue. Catches a per-contract UnknownBackend type (this does not compile then), an
// InProgress without its JobState, an InvalidValue carrying data, a generated dispatch that
// drops a refusal, and an archive that does not round-trip the refusals.
#[test]
fn ordinary_and_meta_refusals_share_unknown_backend_and_dispatch_through_answering() {
    use signal_listener::{Answer, Answerable, Answering, Refused_Data};
    let unknown: UnknownBackend = UnknownBackend(name("missing"));
    let progress = JobState {
        job_id: JobId("job".into()),
        state: State::Waiting,
    };
    // One value of the one shared type fills every refusal that names an unknown backend:
    // Memory's Turning and Applying, and both wire contracts below.
    let _: [Output; 2] = [
        Output::Turning(Turning::UnknownBackend(unknown.clone())),
        Output::Applying(Applying::UnknownBackend(unknown.clone())),
    ];
    struct Fixture {
        unknown: UnknownBackend,
        progress: JobState,
    }
    impl Answering for Fixture {
        fn toggle(&self) -> Result<signal_listener::Toggling, signal_listener::Output> {
            Err(signal_listener::Output::Refused(Refused_Data::NoBackend))
        }
        fn toggle_custom(
            &self,
            _: &Options,
        ) -> Result<signal_listener::Toggling, signal_listener::Output> {
            Err(signal_listener::Output::Refused(
                Refused_Data::UnknownBackend(self.unknown.clone()),
            ))
        }
        fn cancel(&self) -> Result<JobId, signal_listener::Output> {
            Err(signal_listener::Output::Refused(
                Refused_Data::NothingRecording,
            ))
        }
        fn recall(&self) -> Result<Text, signal_listener::Output> {
            Err(signal_listener::Output::Refused(Refused_Data::NothingKept))
        }
        fn recall_job(&self, job_id: &JobId) -> Result<Text, signal_listener::Output> {
            Err(signal_listener::Output::Refused(Refused_Data::NoSuchJob(
                signal_listener::NoSuchJob(job_id.clone()),
            )))
        }
        fn retranscribe(&self, _: &JobId) -> Result<JobId, signal_listener::Output> {
            Err(signal_listener::Output::Refused(Refused_Data::InProgress(
                self.progress.clone(),
            )))
        }
        fn jobs(&self) -> Result<Vec<JobState>, signal_listener::Output> {
            Ok(vec![self.progress.clone()])
        }
        fn history(&self) -> Result<Vec<Past>, signal_listener::Output> {
            Ok(Vec::new())
        }
        fn subscribe(
            &self,
        ) -> Result<signal::Stream<Vec<JobState>, JobChange>, signal_listener::Output> {
            Err(signal_listener::Output::Refused(Refused_Data::Unwritable))
        }
    }
    let fixture = Fixture {
        unknown: unknown.clone(),
        progress: progress.clone(),
    };
    for (input, expected) in [
        (
            signal_listener::Input::ToggleCustom(Options {
                delivery_option: None,
                backend_name_vector_option: Some(vec![name("missing")]),
            }),
            Refused_Data::UnknownBackend(unknown.clone()),
        ),
        (
            signal_listener::Input::Retranscribe(JobId("job".into())),
            Refused_Data::InProgress(progress.clone()),
        ),
    ] {
        let Answer::Once(output) = input.answer(&fixture) else {
            panic!("one-shot input yielded a stream")
        };
        let expected = signal_listener::Output::Refused(expected);
        assert_eq!(output, expected);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&output).unwrap();
        assert_eq!(
            rkyv::from_bytes::<signal_listener::Output, rkyv::rancor::Error>(&bytes).unwrap(),
            expected
        );
    }

    struct Meta {
        unknown: UnknownBackend,
    }
    impl meta_signal_listener::Answering for Meta {
        fn configure(&self, set: &Set) -> Result<Set, meta_signal_listener::Output> {
            Err(meta_signal_listener::Output::Refused(match set {
                Set::Order(_) => {
                    meta_signal_listener::Refused_Data::UnknownBackend(self.unknown.clone())
                }
                _ => meta_signal_listener::Refused_Data::InvalidValue,
            }))
        }
        fn configuration(&self) -> Result<Configuration, meta_signal_listener::Output> {
            Ok(configuration(Sequence::AsReady))
        }
        fn remember(
            &self,
            _: &meta_signal_listener::Remember_Data,
        ) -> Result<JobId, meta_signal_listener::Output> {
            Ok(JobId("remembered".into()))
        }
        fn subscribe(
            &self,
        ) -> Result<signal::Stream<Configuration, Set>, meta_signal_listener::Output> {
            Err(meta_signal_listener::Output::Refused(
                meta_signal_listener::Refused_Data::InvalidValue,
            ))
        }
    }
    use meta_signal_listener::Answerable as _;
    let meta = Meta {
        unknown: unknown.clone(),
    };
    for (set, expected) in [
        (
            Set::Order(vec![name("missing")]),
            meta_signal_listener::Refused_Data::UnknownBackend(unknown.clone()),
        ),
        (
            Set::HistoryLength(HistoryLength(-1)),
            meta_signal_listener::Refused_Data::InvalidValue,
        ),
    ] {
        let meta_signal_listener::Answer::Once(output) =
            meta_signal_listener::Input::Configure(set).answer(&meta)
        else {
            panic!("one-shot input yielded a stream")
        };
        let expected = meta_signal_listener::Output::Refused(expected);
        assert_eq!(output, expected);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&output).unwrap();
        assert_eq!(
            rkyv::from_bytes::<meta_signal_listener::Output, rkyv::rancor::Error>(&bytes).unwrap(),
            expected
        );
    }
}

// RC3/RC4 Datom spelling of the refusals. Catches a renamed refusal, an InvalidValue that
// takes an argument, an InProgress without its JobState, and the pre-repair meta shape in
// which Refused carried the backend name directly.
#[cfg(feature = "datom")]
#[test]
fn datom_spells_the_ruled_refusals() {
    let ordinary: signal_listener::Output = decode("Refused.InProgress.{ job Waiting }").unwrap();
    assert_eq!(
        ordinary,
        signal_listener::Output::Refused(signal_listener::Refused_Data::InProgress(JobState {
            job_id: JobId("job".into()),
            state: State::Waiting,
        }))
    );
    let ordinary: signal_listener::Output = decode("Refused.UnknownBackend.missing").unwrap();
    assert_eq!(
        ordinary,
        signal_listener::Output::Refused(signal_listener::Refused_Data::UnknownBackend(
            UnknownBackend(name("missing"))
        ))
    );
    let meta: meta_signal_listener::Output = decode("Refused.InvalidValue").unwrap();
    assert_eq!(
        meta,
        meta_signal_listener::Output::Refused(meta_signal_listener::Refused_Data::InvalidValue)
    );
    for refused in [
        "Refused.missing",
        "Refused.InvalidValue.missing",
        "Refused.Invalidvalue",
    ] {
        assert!(
            decode::<meta_signal_listener::Output>(refused).is_err(),
            "{refused}"
        );
    }
    assert!(decode::<signal_listener::Output>("Refused.InProgress.job").is_err());
}

// RC11: Pending (the Jobs query and the subscription snapshot) holds every retained job
// that is not Delivered, Failed and Cancelled included, at every commit and across reopen.
// The expected membership is written by hand at each step; how a retirement is announced
// on the stream is not asserted. Catches the pre-repair filter that dropped Failed and
// Cancelled jobs, a Pending that keeps Delivered or imported jobs, a requeued job that does
// not return, and a job retired from the store that stays listed.
#[test]
fn pending_lists_every_retained_job_but_delivered_ones_across_transitions_and_reopen() {
    use std::collections::BTreeMap;
    fn expect(memory: &listener_memory::Memory, jobs: &[(&str, State)]) {
        let expected: BTreeMap<String, State> = jobs
            .iter()
            .map(|(id, state)| ((*id).to_string(), state.clone()))
            .collect();
        assert_eq!(standing(memory), expected);
        let retained: BTreeMap<String, State> = memory
            .inspect()
            .unwrap()
            .job_vector
            .iter()
            .filter(|job| job.state != State::Delivered)
            .map(|job| (job.job_id.0.clone(), job.state.clone()))
            .collect();
        assert_eq!(retained, expected);
    }
    use State::{Cancelled, Failed, Recording, Transcribing, Waiting};
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsReady);
    expect(&memory, &[]);
    start(&memory, "failed", 1);
    expect(&memory, &[("failed", Recording)]);
    stop(&memory, "failed");
    expect(&memory, &[("failed", Transcribing)]);
    exhaust(&memory, "failed", Vec::new());
    expect(&memory, &[("failed", Failed)]);
    start(&memory, "cancelled", 2);
    execute(&memory, Input::Drop, 2, None);
    expect(&memory, &[("failed", Failed), ("cancelled", Cancelled)]);
    capture(&memory, "delivered", 3, "spoken");
    import(&memory, "imported", 4, "remembered");
    expect(&memory, &[("failed", Failed), ("cancelled", Cancelled)]);
    start(&memory, "waiting", 5);
    stop(&memory, "waiting");
    heard(&memory, "waiting", "ready");
    start(&memory, "recording", 6);
    let everything = [
        ("failed", Failed),
        ("cancelled", Cancelled),
        ("waiting", Waiting),
        ("recording", Recording),
    ];
    expect(&memory, &everything);
    execute(&memory, Input::Requeue(JobId("delivered".into())), 7, None);
    execute(&memory, Input::Requeue(JobId("failed".into())), 7, None);
    expect(
        &memory,
        &[
            ("failed", Transcribing),
            ("cancelled", Cancelled),
            ("delivered", Transcribing),
            ("waiting", Waiting),
            ("recording", Recording),
        ],
    );
    exhaust(&memory, "failed", Vec::new());
    heard(&memory, "delivered", "spoken again");
    assert_eq!(
        finish(&memory, "waiting"),
        Next::Deliverable(ready("delivered", "spoken again", Delivery::Kept))
    );
    assert_eq!(finish(&memory, "delivered"), Next::Nothing);
    expect(
        &memory,
        &[
            ("failed", Failed),
            ("cancelled", Cancelled),
            ("recording", Recording),
        ],
    );
    apply(&memory, Set::AudioKept(Retention::Days(Days(0))));
    execute(&memory, Input::Prune, 10, None);
    expect(
        &memory,
        &[
            ("failed", Failed),
            ("cancelled", Cancelled),
            ("recording", Recording),
        ],
    );
    execute(
        &memory,
        Input::Forget(vec![location("/fixture/audio-before", "cancelled")]),
        10,
        None,
    );
    expect(&memory, &[("failed", Failed), ("recording", Recording)]);
    drop(memory);
    let memory = reopen(&directory);
    expect(&memory, &[("failed", Failed), ("recording", Recording)]);
    execute(&memory, Input::Recover, 11, None);
    expect(&memory, &[("failed", Failed), ("recording", Transcribing)]);
    execute(
        &memory,
        Input::Forget(vec![location("/fixture/audio-before", "failed")]),
        12,
        None,
    );
    expect(&memory, &[("recording", Transcribing)]);
}

fn every_state() -> [State; 6] {
    [
        State::Recording,
        State::Transcribing,
        State::Waiting,
        State::Delivered,
        State::Failed,
        State::Cancelled,
    ]
}
// Both JobChange variants: a JobState for every State, and Forgotten.
fn every_job_change() -> Vec<JobChange> {
    every_state()
        .into_iter()
        .map(|state| {
            JobChange::JobState(JobState {
                job_id: JobId("job".into()),
                state,
            })
        })
        .chain([JobChange::Forgotten(JobId("job".into()))])
        .collect()
}
fn every_subscribed_frame() -> Vec<signal::Frame<Vec<JobState>, JobChange>> {
    use signal::Frame;
    let jobs: Vec<JobState> = every_state()
        .into_iter()
        .enumerate()
        .map(|(index, state)| JobState {
            job_id: JobId(format!("job-{index}")),
            state,
        })
        .collect();
    [
        Frame::Opened(jobs.clone()),
        Frame::Opened(Vec::new()),
        Frame::Lagged(jobs),
    ]
    .into_iter()
    .chain(every_job_change().into_iter().map(Frame::Changed))
    .collect()
}

// The ordinary JobChange and the Subscribed frame through the Signal archive. Catches a
// JobChange whose variants are confused or lose their payload (Forgotten read back as a
// JobState, a JobState's State swapped), an Opened, Lagged or Changed frame that does not
// survive the archive or comes back as another frame, and a reader that accepts a variant tag
// outside JobChange's two, a truncated archive, or an archive followed by further bytes.
#[test]
fn job_change_and_subscribed_frames_survive_the_signal_archive_and_corrupt_bytes_are_refused() {
    use signal::{ByteViewable, Restorable, Signal, Signalizable};
    let changes = every_job_change();
    for change in &changes {
        let bytes = change.signalize().unwrap().bytes().to_vec();
        assert_eq!(
            Signal::<JobChange>::from(bytes.clone()).restore().unwrap(),
            *change
        );
        // The archive's root is its last bytes, and an enum's tag is the root's first byte.
        let tag = bytes.len() - std::mem::size_of::<rkyv::Archived<JobChange>>();
        assert!(bytes[tag] < 2, "{bytes:?}");
        for wrong in 2..=u8::MAX {
            let mut corrupt = bytes.clone();
            corrupt[tag] = wrong;
            assert!(
                Signal::<JobChange>::from(corrupt).restore().is_err(),
                "tag {wrong} accepted"
            );
        }
        assert!(
            Signal::<JobChange>::from(bytes[..bytes.len() - 1].to_vec())
                .restore()
                .is_err()
        );
        for trailing in 0..=u8::MAX {
            let mut longer = bytes.clone();
            longer.push(trailing);
            assert!(
                Signal::<JobChange>::from(longer).restore().is_err(),
                "a trailing {trailing} accepted"
            );
        }
    }
    for frame in every_subscribed_frame() {
        let output = signal_listener::Output::Subscribed(frame);
        let bytes = output.signalize().unwrap().bytes().to_vec();
        assert_eq!(
            Signal::<signal_listener::Output>::from(bytes)
                .restore()
                .unwrap(),
            output
        );
    }
}

// The ruled Datom spelling of JobChange and of the Subscribed frames, and their Datom
// round-trip. Catches a renamed variant (the State or Retired forms Fable ruled out), a
// JobState variant that does not carry the JobState struct, a Forgotten that carries more
// than its JobId, the pre-retirement Subscribed change that was a bare JobState, and a reader
// that accepts trailing text.
#[cfg(feature = "datom")]
#[test]
fn datom_spells_job_change_and_subscribed_frames_by_the_ruled_names() {
    use datom_codec::{Composing, Datomizable};
    use signal::Frame;
    let budget = || datom_codec::Budget {
        remaining: 1024,
        reader: 4096_usize.try_into().unwrap(),
        depth: 0,
        maximum_depth: 64,
    };
    for (state, spelled) in every_state().into_iter().zip([
        "Recording",
        "Transcribing",
        "Waiting",
        "Delivered",
        "Failed",
        "Cancelled",
    ]) {
        let change = JobChange::JobState(JobState {
            job_id: JobId("job".into()),
            state,
        });
        assert_eq!(
            decode::<JobChange>(&format!("JobState.{{ job {spelled} }}")).unwrap(),
            change
        );
        assert_eq!(
            decode::<signal_listener::Output>(&format!(
                "Subscribed.Changed.JobState.{{ job {spelled} }}"
            ))
            .unwrap(),
            signal_listener::Output::Subscribed(Frame::Changed(change))
        );
    }
    assert_eq!(
        decode::<JobChange>("Forgotten.job").unwrap(),
        JobChange::Forgotten(JobId("job".into()))
    );
    assert_eq!(
        decode::<signal_listener::Output>("Subscribed.Changed.Forgotten.job").unwrap(),
        signal_listener::Output::Subscribed(Frame::Changed(JobChange::Forgotten(JobId(
            "job".into()
        ))))
    );
    assert_eq!(
        decode::<signal_listener::Output>(
            "Subscribed.Opened.[ { first Waiting } { second Failed } ]"
        )
        .unwrap(),
        signal_listener::Output::Subscribed(Frame::Opened(vec![
            JobState {
                job_id: JobId("first".into()),
                state: State::Waiting,
            },
            JobState {
                job_id: JobId("second".into()),
                state: State::Failed,
            },
        ]))
    );
    for change in every_job_change() {
        let datom = change.datomize(Vec::new());
        assert_eq!(JobChange::compose(&datom, &mut budget()).unwrap(), change);
    }
    for frame in every_subscribed_frame() {
        let output = signal_listener::Output::Subscribed(frame);
        let datom = output.datomize(Vec::new());
        assert_eq!(
            signal_listener::Output::compose(&datom, &mut budget()).unwrap(),
            output
        );
    }
    for wrong in [
        "State.{ job Waiting }",
        "Retired.job",
        "JobState.job",
        "JobState.{ job Waiting Failed }",
        "Forgotten.{ job Waiting }",
        "Forgotten.job job",
        "JobState.{ job Waiting } extra",
    ] {
        assert!(decode::<JobChange>(wrong).is_err(), "{wrong}");
    }
    for wrong in [
        "Subscribed.Changed.{ job Waiting }",
        "Subscribed.Changed.Retired.job",
        "Subscribed.Changed.Forgotten.job extra",
        "Subscribed.Opened.[ Forgotten.job ]",
    ] {
        assert!(decode::<signal_listener::Output>(wrong).is_err(), "{wrong}");
    }
}

// The InProgress invariant: Requeue refuses every job that is Recording, Transcribing or
// Waiting with InProgress carrying its current JobState, however the job reached that state,
// and requeues every terminal job with kept audio. Catches a Requeue that resets an in-progress
// job reached by a route other than the plain one (Interrupt, Recover after reopen, an earlier
// Requeue, a Waiting job held behind an earlier one under AsRecorded), an InProgress carrying
// a stale state, a refusal that commits a change, and a terminal state refused. Requeue of a
// Cancelled job is accepted on the reading that InProgress is ruled for Recording, Transcribing
// and Waiting alone (the producer seams), with vision/listener/1.md's "Retranscribe hears a
// job's kept audio anew"; no ruling names Cancelled itself.
#[test]
fn requeue_refuses_every_in_progress_state_however_reached_and_requeues_every_terminal_state() {
    fn in_progress(memory: &listener_memory::Memory, id: &str, state: State) {
        assert_eq!(
            observe(memory, Input::Requeue(JobId(id.into())), 20),
            Output::Requeueing(Requeueing::InProgress(JobState {
                job_id: JobId(id.into()),
                state,
            })),
            "{id}"
        );
    }
    let directory = TempDir::new().unwrap();
    let memory = create(&directory, Sequence::AsRecorded);
    capture(&memory, "delivered", 1, "spoken");
    start(&memory, "failed", 2);
    stop(&memory, "failed");
    exhaust(&memory, "failed", Vec::new());
    start(&memory, "cancelled", 3);
    execute(&memory, Input::Drop, 3, None);
    start(&memory, "stopped", 4);
    stop(&memory, "stopped");
    start(&memory, "interrupted", 5);
    execute(
        &memory,
        Input::Interrupt(JobId("interrupted".into())),
        5,
        None,
    );
    start(&memory, "held", 6);
    stop(&memory, "held");
    assert_eq!(heard(&memory, "held", "held text"), Next::Nothing);
    start(&memory, "recording", 7);
    in_progress(&memory, "recording", State::Recording);
    in_progress(&memory, "stopped", State::Transcribing);
    in_progress(&memory, "interrupted", State::Transcribing);
    in_progress(&memory, "held", State::Waiting);
    for id in ["delivered", "failed", "cancelled"] {
        assert_eq!(
            changed(
                &memory,
                Input::Requeue(JobId(id.into())),
                8,
                None,
                changes(&[(id, State::Transcribing)], &[]),
            ),
            Output::Requeueing(Requeueing::Requeued(JobId(id.into())))
        );
        in_progress(&memory, id, State::Transcribing);
    }
    assert_eq!(
        standing(&memory).get("cancelled"),
        Some(&State::Transcribing)
    );
    drop(memory);
    let memory = reopen(&directory);
    execute(&memory, Input::Recover, 9, None);
    in_progress(&memory, "recording", State::Transcribing);
    assert_eq!(
        observe(&memory, Input::Tries(JobId("cancelled".into())), 9),
        trial(vec![backend("speech", "model")])
    );
    assert_eq!(heard(&memory, "cancelled", "heard anew"), Next::Nothing);
    in_progress(&memory, "cancelled", State::Waiting);
}

// UNRULED: duplicate names in Order. No ruling or contract text says whether an Order may
// name a backend twice; the producer accepts it on create and on Set::Order and keeps it as
// written. This pins that current behavior so a change to it is seen; it is not a ruled
// oracle. It catches an Order silently deduplicated or reordered on create, Set or reopen.
#[test]
fn duplicate_order_names_are_kept_as_written_on_create_set_and_reopen_unruled() {
    let directory = TempDir::new().unwrap();
    let written = ["speech", "whisper", "speech"];
    let memory = definition(&directory)
        .create(configured(
            Sequence::AsReady,
            &["speech", "whisper"],
            &written,
        ))
        .unwrap();
    assert_eq!(
        settings(&memory).order,
        Order(written.iter().map(|text| name(text)).collect())
    );
    let twice = vec![name("whisper"), name("whisper")];
    apply(&memory, Set::Order(twice.clone()));
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(settings(&memory).order, Order(twice.clone()));
    start(&memory, "job", 1);
    assert_eq!(
        job(&memory.inspect().unwrap(), "job").backend_name_vector,
        twice
    );
}

// Removing a backend takes its name out of the configured Order and keeps every other name in
// its order; an Order that leaves a backend out keeps that backend configured. Catches a
// RemoveBackend that clears, rebuilds or reorders the Order, one that changes an Order which
// did not name the removed backend, a Set::Order that deletes the backends it leaves out, a
// later Turn that does not use the remaining Order, and an Order naming the removed backend
// accepted afterwards.
#[test]
fn removing_a_backend_keeps_the_rest_of_the_configured_order_in_its_order() {
    let directory = TempDir::new().unwrap();
    let memory = definition(&directory)
        .create(configured(
            Sequence::AsReady,
            &["first", "second", "third", "fourth"],
            &["fourth", "second", "first"],
        ))
        .unwrap();
    apply(&memory, Set::RemoveBackend(name("second")));
    let configuration = settings(&memory);
    assert_eq!(
        configuration.order,
        Order(vec![name("fourth"), name("first")])
    );
    assert_eq!(
        configuration.backend_vector,
        vec![
            backend("first", "model"),
            backend("third", "model"),
            backend("fourth", "model")
        ]
    );
    apply(&memory, Set::RemoveBackend(name("third")));
    assert_eq!(
        settings(&memory).order,
        Order(vec![name("fourth"), name("first")])
    );
    assert_eq!(
        observe(
            &memory,
            Input::Apply(Set::Order(vec![name("first"), name("second")])),
            1
        ),
        Output::Applying(Applying::UnknownBackend(UnknownBackend(name("second"))))
    );
    apply(&memory, Set::Order(vec![name("first")]));
    let configuration = settings(&memory);
    assert_eq!(configuration.order, Order(vec![name("first")]));
    assert_eq!(
        configuration.backend_vector,
        vec![backend("first", "model"), backend("fourth", "model")]
    );
    drop(memory);
    let memory = reopen(&directory);
    assert_eq!(settings(&memory), configuration);
    start(&memory, "job", 2);
    assert_eq!(
        job(&memory.inspect().unwrap(), "job").backend_name_vector,
        vec![name("first")]
    );
}

// The failed-commit fixture. A child process opens an existing store under a file-size limit
// near its size, with SIGXFSZ ignored, and imports a text far larger than the limit, so the
// kernel's commit fails with EFBIG. The child answers whether execute reported the commit.
const COMMIT_PATH: &str = "LISTENER_TESTS_COMMIT_PATH";
const FAILED_COMMIT: i32 = 71;
#[test]
#[ignore = "child process of the failed-commit fixture; its parent names the store"]
fn commit_child() {
    let path = std::path::PathBuf::from(std::env::var_os(COMMIT_PATH).unwrap());
    let memory = definition_at(&path).open().unwrap();
    let result = memory.execute(request(
        Input::Import(Import_Data {
            moment: Moment(9),
            text: Text("x".repeat(16 << 20)),
        }),
        9,
        Some("large"),
    ));
    std::process::exit(if result.is_ok() { 0 } else { FAILED_COMMIT });
}

// A commit the kernel refuses escapes as an error and changes nothing: execute returns no
// Committed (so no change reaches a subscriber) and the store reopens exactly as it was, still
// committing. Catches an execute that ignores the kernel's write result and reports the
// changes anyway, and a failed commit that leaves the store damaged or partly written.
#[test]
fn a_commit_the_kernel_refuses_reports_nothing_and_leaves_the_store_as_it_was() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("state.memory");
    let memory = definition_at(&path)
        .create(configuration(Sequence::AsReady))
        .unwrap();
    capture(&memory, "kept", 1, "kept text");
    start(&memory, "active", 2);
    let state = memory.inspect().unwrap();
    drop(memory);
    let blocks = std::fs::metadata(&path).unwrap().len() / 512 + 16;
    let status = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            "ulimit -c 0; trap '' XFSZ; ulimit -f \"$1\" && exec \"$2\" --ignored --exact commit_child --test-threads 1",
            "sh",
        ])
        .arg(blocks.to_string())
        .arg(std::env::current_exe().unwrap())
        .env(COMMIT_PATH, &path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(FAILED_COMMIT), "{status}");
    let memory = definition_at(&path).open().unwrap();
    assert_eq!(memory.inspect().unwrap(), state);
    assert_eq!(
        standing(&memory),
        JobView::from([("active".to_string(), State::Recording)])
    );
    import(&memory, "after", 3, "after text");
}

// The publication crash fixture: the creation child runs under strace, which kills it with
// SIGKILL on entry to the chosen invocation of one system call, so the call never runs. The
// sweep kills at every linkat (publication is a hard link), every fsync (the staged file's
// sync, then the parent directory's after the link) and every fdatasync (the kernel's
// commits), until a run completes.
#[derive(Debug)]
struct InjectedRun {
    syscall: &'static str,
    invocation: u32,
    ending: Ending,
    final_path: FinalPath,
    staged: Vec<Stage>,
    parent: TempDir,
}
fn injected_runs(creator: &str, syscall: &'static str) -> Vec<InjectedRun> {
    use std::os::unix::process::ExitStatusExt;
    let executable = std::env::current_exe().unwrap();
    let mut runs = Vec::new();
    for invocation in 1..=64 {
        let parent = TempDir::new().unwrap();
        let path = parent.path().join("state.memory");
        let status = std::process::Command::new("strace")
            .args(["-f", "-qq", "-o", "/dev/null"])
            .arg(format!("--trace={syscall}"))
            .arg(format!("--inject={syscall}:signal=KILL:when={invocation}"))
            .arg(&executable)
            .args([
                "--ignored",
                "--exact",
                "creation_child",
                "--test-threads",
                "1",
            ])
            .env(CREATION_PATH, &path)
            .env(CREATOR, creator)
            .current_dir(parent.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        let ending = match (status.code(), status.signal()) {
            (Some(0), _) => Ending::Completed,
            (None, Some(9)) | (Some(137), _) => Ending::Killed,
            _ => panic!(
                "creation child under strace ended otherwise at {syscall} {invocation}: {status}"
            ),
        };
        let staged = std::fs::read_dir(parent.path())
            .unwrap()
            .map(|entry| entry.unwrap().path().join("state.memory"))
            .filter(|staged| staged.is_file())
            .map(|staged| stage(&staged))
            .collect();
        let completed = ending == Ending::Completed;
        let final_path = final_path(&path);
        // The sweep's own record, shown by the check that runs it with --nocapture.
        eprintln!(
            "{creator} {syscall} {invocation}: {ending:?}, final {final_path:?}, staged {staged:?}"
        );
        runs.push(InjectedRun {
            syscall,
            invocation,
            ending,
            final_path,
            staged,
            parent,
        });
        if completed {
            return runs;
        }
    }
    panic!("no creation completed under {syscall} injection: {runs:?}")
}

// RC1, the window a file-size limit cannot reach: a process killed after the record commits
// and before publication, or after publication and before the directory sync (the executor's
// own seam for it, store.rs's exit_after_initial_commit, is a private unit test and is not
// reachable from here; strace kills the real create at the system call instead). Every killed
// creation leaves the final path absent (open answers MissingStore and a retry creates a
// complete store) or complete (it opens with the created configuration and commits); never
// stamped-only, empty or damaged. The sweep must show both windows: a kill at linkat with the
// staged file Recorded and the final path absent, and a kill after the link with the final path
// complete. Catches a creation that links or renames before the record commits, one that
// publishes an incomplete file, and one whose interrupted staging blocks a later creation.
// Needs strace, so it is ignored here and run by the flake's publication-crash check.
#[test]
#[ignore = "needs strace; run by checks.publication-crash"]
fn a_crash_after_the_record_commits_and_before_publication_leaves_the_final_path_absent_or_complete()
 {
    let runs: Vec<InjectedRun> = ["linkat", "fsync", "fdatasync"]
        .into_iter()
        .flat_map(|syscall| injected_runs("producer", syscall))
        .collect();
    assert!(
        runs.iter().any(|run| run.syscall == "linkat"
            && run.ending == Ending::Killed
            && run.final_path == FinalPath::Absent
            && run.staged == [Stage::Recorded]),
        "no kill between the record's commit and publication: {runs:?}"
    );
    assert!(
        runs.iter()
            .any(|run| run.ending == Ending::Killed && run.final_path == FinalPath::Complete),
        "no kill after publication: {runs:?}"
    );
    for run in &runs {
        let path = run.parent.path().join("state.memory");
        match run.final_path {
            FinalPath::Absent => {
                assert_ne!(
                    run.ending,
                    Ending::Completed,
                    "{} {} completed without publishing",
                    run.syscall,
                    run.invocation
                );
                assert!(
                    matches!(definition_at(&path).open(), Err(Error::MissingStore)),
                    "{run:?}"
                );
                let retried = definition_at(&path)
                    .create(configuration(Sequence::AsRecorded))
                    .unwrap();
                start(&retried, "after", 1);
                let state = retried.inspect().unwrap();
                drop(retried);
                assert_eq!(
                    definition_at(&path).open().unwrap().inspect().unwrap(),
                    state
                );
            }
            FinalPath::Complete => {
                let memory = definition_at(&path).open().unwrap();
                import(&memory, "after", 1, "after text");
                let state = memory.inspect().unwrap();
                drop(memory);
                assert_eq!(
                    definition_at(&path).open().unwrap().inspect().unwrap(),
                    state
                );
            }
            FinalPath::Incomplete(_) => panic!("an incomplete final path: {run:?}"),
        }
    }
    // The fixture's own discrimination: the deliberately wrong direct creator, killed at its
    // kernel commits, leaves an incomplete final path, so the sweep reaches a window that
    // matters and the assertions above cannot pass vacuously.
    assert!(
        injected_runs("direct", "fdatasync")
            .iter()
            .any(|run| run.ending == Ending::Killed
                && matches!(run.final_path, FinalPath::Incomplete(_))),
        "the sweep never caught the direct creator"
    );
}
