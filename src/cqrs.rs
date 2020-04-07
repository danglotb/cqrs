use std::marker::PhantomData;
use std::rc::Rc;
use crate::aggregate::{AggregateId, Aggregate, AggregateError};
use crate::event::{DomainEvent, MessageEnvelope};
use crate::store::EventStore;
use crate::config::MetadataSupplier;
use crate::view::ViewProcessor;
use crate::command::Command;

/// This is the base framework for applying commands to produce events.
pub struct CqrsFramework<I, A, E, ES, M>
    where
        I: AggregateId<A>,
        A: Aggregate,
        E: DomainEvent<A>,
        ES: EventStore<I, A, E>,
        M: MetadataSupplier
{
    store: ES,
    view: Rc<dyn ViewProcessor<I, A, E>>,
    metadata_supplier: M,
    _phantom: PhantomData<I>,
}

impl<I, A, E, ES, M> CqrsFramework<I, A, E, ES, M>
    where
        I: AggregateId<A>,
        A: Aggregate,
        E: DomainEvent<A>,
        ES: EventStore<I, A, E>,
        M: MetadataSupplier
{
    /// Creates new framework for dispatching commands using the provided elements.
    pub fn new(store: ES, view: Rc<dyn ViewProcessor<I, A, E>>, metadata_supplier: M) -> CqrsFramework<I, A, E, ES, M>
        where
            I: AggregateId<A>,
            A: Aggregate,
            E: DomainEvent<A>,
            ES: EventStore<I, A, E>,
            M: MetadataSupplier
    {
        CqrsFramework {
            store,
            view,
            metadata_supplier,
            _phantom: PhantomData::<I>,
        }
    }

    /// This applies a command to an aggregate, this is the only way to make any change to
    /// the state of an aggregate.
    ///
    /// An error while processing will result in no events committed and
    /// an AggregateError being returned.
    ///
    /// If successful the events produced will be applied to the [`ViewProcessor`].
    pub fn execute<C: Command<A, E>>(&self, aggregate_id: &I, command: C) -> Result<(), AggregateError> {
        let (mut aggregate, current_sequence) = self.load_aggregate(aggregate_id);
        let resultant_events = command.handle(&mut aggregate)?;
        let wrapped_events = self.wrap_events(aggregate_id, current_sequence, resultant_events);

        let committed_events = <CqrsFramework<I, A, E, ES, M>>::duplicate(&wrapped_events);
        self.store.commit(wrapped_events)?;
        self.view.dispatch(&aggregate_id, committed_events);
        Ok(())
    }

    fn duplicate(wrapped_events: &[MessageEnvelope<A, E>]) -> Vec<MessageEnvelope<A, E>> {
        let mut committed_events = Vec::new();
        for wrapped_event in wrapped_events {
            committed_events.push((*wrapped_event).clone());
        }
        committed_events
    }

    fn wrap_events(&self, aggregate_id: &I, current_sequence: usize, resultant_events: Vec<E>) -> Vec<MessageEnvelope<A, E>> {
        let mut sequence = current_sequence;
        let mut wrapped_events: Vec<MessageEnvelope<A, E>> = Vec::new();
        for payload in resultant_events {
            sequence += 1;
            let aggregate_type = aggregate_id.aggregate_type().to_string();
            let aggregate_id: String = aggregate_id.to_string();
            let sequence = sequence;
            let metadata = self.metadata_supplier.supply();
            wrapped_events.push(MessageEnvelope {
                aggregate_id,
                sequence,
                aggregate_type,
                payload,
                metadata,
                _phantom: PhantomData,
            });
        }
        wrapped_events
    }

    fn load_aggregate(&self, aggregate_id: &I) -> (A, usize) {
        let committed_events = self.store.load(aggregate_id);
        let mut aggregate = A::default();
        let mut current_sequence = 0;
        for envelope in committed_events {
            current_sequence = envelope.sequence;
            let event = envelope.payload;
            event.apply(&mut aggregate);
        }
        (aggregate, current_sequence)
    }
}