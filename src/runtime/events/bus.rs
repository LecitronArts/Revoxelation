use std::collections::VecDeque;

use super::command::{CommandEnvelope, RuntimeCommand};
use super::event::{CommandOutcome, CommandOutcomeEvent, EventEnvelope, RuntimeEvent};
use super::sequence::{SequenceClock, SequenceMetadata, is_monotonic};
use super::validation::validate;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventBusSnapshot {
    pub frame_index: u64,
    pub emitted_events: Vec<EventEnvelope>,
    pub consumed_events: Vec<EventEnvelope>,
}

impl EventBusSnapshot {
    pub fn emitted_sequences(&self) -> Vec<SequenceMetadata> {
        self.emitted_events
            .iter()
            .map(|entry| entry.sequence)
            .collect()
    }

    pub fn consumed_sequences(&self) -> Vec<SequenceMetadata> {
        self.consumed_events
            .iter()
            .map(|entry| entry.sequence)
            .collect()
    }

    pub fn emitted_is_monotonic(&self) -> bool {
        is_monotonic(&self.emitted_sequences())
    }
}

#[derive(Debug, Clone)]
pub struct EventBus {
    frame_index: u64,
    sequence_clock: SequenceClock,
    pending_commands: VecDeque<CommandEnvelope>,
    pending_events: VecDeque<EventEnvelope>,
    emitted_events: Vec<EventEnvelope>,
    consumed_events: Vec<EventEnvelope>,
}

impl EventBus {
    pub fn new(frame_index: u64) -> Self {
        Self {
            frame_index,
            sequence_clock: SequenceClock::new(frame_index),
            pending_commands: VecDeque::new(),
            pending_events: VecDeque::new(),
            emitted_events: Vec::new(),
            consumed_events: Vec::new(),
        }
    }

    pub fn publish_command(&mut self, command: RuntimeCommand) -> SequenceMetadata {
        let sequence = self.sequence_clock.next_seq();
        self.pending_commands
            .push_back(CommandEnvelope { sequence, command });
        sequence
    }

    pub fn process_pending_commands(&mut self) {
        while let Some(envelope) = self.pending_commands.pop_front() {
            let command_kind = envelope.kind();
            match validate(&envelope.command) {
                Ok(()) => {
                    self.emit_event(RuntimeEvent::CommandOutcome(CommandOutcomeEvent {
                        command_kind,
                        outcome: CommandOutcome::Accepted,
                    }));
                    self.emit_event(Self::acceptance_event(envelope.command));
                }
                Err(reason) => {
                    self.emit_event(RuntimeEvent::CommandOutcome(CommandOutcomeEvent {
                        command_kind,
                        outcome: CommandOutcome::Rejected { reason },
                    }));
                }
            }
        }
    }

    pub fn consume_emitted(&mut self) -> Vec<EventEnvelope> {
        let mut consumed_now = Vec::with_capacity(self.pending_events.len());
        while let Some(event) = self.pending_events.pop_front() {
            consumed_now.push(event.clone());
            self.consumed_events.push(event);
        }
        consumed_now
    }

    pub fn snapshot(&self) -> EventBusSnapshot {
        EventBusSnapshot {
            frame_index: self.frame_index,
            emitted_events: self.emitted_events.clone(),
            consumed_events: self.consumed_events.clone(),
        }
    }

    fn emit_event(&mut self, event: RuntimeEvent) {
        let envelope = EventEnvelope {
            sequence: self.sequence_clock.next_seq(),
            event,
        };
        self.pending_events.push_back(envelope.clone());
        self.emitted_events.push(envelope);
    }

    fn acceptance_event(command: RuntimeCommand) -> RuntimeEvent {
        match command {
            RuntimeCommand::PlayerAction(payload) => RuntimeEvent::PlayerActionApplied(payload),
            RuntimeCommand::ChunkLifecycle(payload) => RuntimeEvent::ChunkLifecycleApplied(payload),
            RuntimeCommand::BlockEdit(payload) => RuntimeEvent::BlockEditApplied(payload),
        }
    }
}
