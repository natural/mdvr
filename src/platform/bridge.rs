use std::collections::VecDeque;

use crate::contracts::{
    ActionMessage, ActionMessageEnvelope, ContractError, DocumentId, Generation, Message,
    NavigationRequest, PositionCaptured, RenderError, RenderReady, ResourceRequest, decode,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BridgeContext {
    pub document: Option<DocumentId>,
    pub generation: Option<Generation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BridgeError {
    Contract(ContractError),
    NotAction,
    StaleContext {
        expected: BridgeContext,
        actual: BridgeContext,
    },
    UnsupportedAction,
}

impl From<ContractError> for BridgeError {
    fn from(error: ContractError) -> Self {
        Self::Contract(error)
    }
}

/// Pure bridge boundary: decode, validate context, filter closed actions, queue.
pub(crate) struct BridgeRouter {
    context: BridgeContext,
    queue: VecDeque<BridgeMessage>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BridgeMessage {
    Action(ActionMessageEnvelope),
    Navigation(NavigationRequest),
    Resource(ResourceRequest),
    PositionCaptured(PositionCaptured),
    RenderReady(RenderReady),
    RenderError(RenderError),
}

impl BridgeRouter {
    pub(crate) fn new(context: BridgeContext) -> Self {
        Self {
            context,
            queue: VecDeque::new(),
        }
    }

    pub(crate) fn accept(&mut self, bytes: &[u8]) -> Result<(), BridgeError> {
        let envelope = decode(bytes)?;
        let message = match envelope.message {
            Message::Action(action) => {
                let actual = BridgeContext {
                    document: action.document,
                    generation: action.generation,
                };
                if actual != self.context {
                    return Err(BridgeError::StaleContext {
                        expected: self.context,
                        actual,
                    });
                }
                if !is_supported(&action.action) {
                    return Err(BridgeError::UnsupportedAction);
                }
                BridgeMessage::Action(action)
            }
            Message::NavigationRequest(request) => {
                let actual = BridgeContext {
                    document: Some(request.document),
                    generation: Some(request.generation),
                };
                if actual != self.context {
                    return Err(BridgeError::StaleContext {
                        expected: self.context,
                        actual,
                    });
                }
                BridgeMessage::Navigation(request)
            }
            Message::PositionCaptured(position) => {
                let actual = BridgeContext {
                    document: Some(position.document),
                    generation: Some(position.generation),
                };
                if actual != self.context {
                    return Err(BridgeError::StaleContext {
                        expected: self.context,
                        actual,
                    });
                }
                BridgeMessage::PositionCaptured(position)
            }
            Message::RenderReady(ready) => {
                let actual = BridgeContext {
                    document: Some(ready.document),
                    generation: Some(ready.generation),
                };
                if actual != self.context {
                    return Err(BridgeError::StaleContext {
                        expected: self.context,
                        actual,
                    });
                }
                BridgeMessage::RenderReady(ready)
            }
            Message::RenderError(error) => {
                let actual = BridgeContext {
                    document: Some(error.document),
                    generation: Some(error.generation),
                };
                if actual != self.context {
                    return Err(BridgeError::StaleContext {
                        expected: self.context,
                        actual,
                    });
                }
                BridgeMessage::RenderError(error)
            }
            Message::ResourceRequest(request) => {
                let actual = BridgeContext {
                    document: Some(request.document),
                    generation: Some(request.generation),
                };
                if actual != self.context {
                    return Err(BridgeError::StaleContext {
                        expected: self.context,
                        actual,
                    });
                }
                BridgeMessage::Resource(request)
            }
            _ => return Err(BridgeError::NotAction),
        };

        self.queue.push_back(message);
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.queue.clear();
    }

    pub(crate) fn set_context(&mut self, context: BridgeContext) {
        if context != self.context {
            self.queue.clear();
            self.context = context;
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<BridgeMessage> {
        self.queue.drain(..).collect()
    }

    pub(crate) fn drain_actions(&mut self) -> Vec<ActionMessageEnvelope> {
        self.drain()
            .into_iter()
            .filter_map(|message| match message {
                BridgeMessage::Action(action) => Some(action),
                BridgeMessage::Navigation(_)
                | BridgeMessage::Resource(_)
                | BridgeMessage::PositionCaptured(_)
                | BridgeMessage::RenderReady(_)
                | BridgeMessage::RenderError(_) => None,
            })
            .collect()
    }

    pub(crate) fn drain_navigation(&mut self) -> Vec<NavigationRequest> {
        self.drain()
            .into_iter()
            .filter_map(|message| match message {
                BridgeMessage::Action(_)
                | BridgeMessage::Resource(_)
                | BridgeMessage::PositionCaptured(_)
                | BridgeMessage::RenderReady(_)
                | BridgeMessage::RenderError(_) => None,
                BridgeMessage::Navigation(request) => Some(request),
            })
            .collect()
    }
}

fn is_supported(action: &ActionMessage) -> bool {
    matches!(
        action,
        ActionMessage::Search(_)
            | ActionMessage::Copy(_)
            | ActionMessage::SelectAll
            | ActionMessage::Outline(_)
            | ActionMessage::Focus(_)
            | ActionMessage::TextScale(_)
            | ActionMessage::History(_)
            | ActionMessage::Theme(_)
            | ActionMessage::Open(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        ActionMessage, CopyAction, Envelope, FocusOwner, Locator, LocatorFallback, RequestId,
        ResourceId, ResourceKind, ResourceReference, SearchAction, encode,
    };

    fn id(value: u64) -> RequestId {
        RequestId::new(value).unwrap()
    }

    fn bytes(request: u64, document: Option<DocumentId>, action: ActionMessage) -> Vec<u8> {
        bytes_with_generation(request, document, None, action)
    }

    fn bytes_with_generation(
        request: u64,
        document: Option<DocumentId>,
        generation: Option<Generation>,
        action: ActionMessage,
    ) -> Vec<u8> {
        encode(&Envelope::new(Message::Action(ActionMessageEnvelope {
            request: id(request),
            document,
            generation,
            action,
        })))
        .unwrap()
    }

    #[test]
    fn accepts_valid_action_and_drains_envelope() {
        let mut router = BridgeRouter::new(BridgeContext::default());
        let expected = ActionMessageEnvelope {
            request: id(1),
            document: None,
            generation: None,
            action: ActionMessage::Search(SearchAction::Next),
        };

        router
            .accept(&bytes(1, None, ActionMessage::Search(SearchAction::Next)))
            .unwrap();

        assert_eq!(router.drain_actions(), vec![expected]);
    }

    #[test]
    fn accepts_context_bound_resource_requests() {
        let context = BridgeContext {
            document: Some(DocumentId::new(4).unwrap()),
            generation: Some(Generation::new(7).unwrap()),
        };
        let mut router = BridgeRouter::new(context);
        let request = ResourceRequest {
            request: id(2),
            resource: ResourceId::new(3).unwrap(),
            document: context.document.unwrap(),
            generation: context.generation.unwrap(),
            kind: ResourceKind::Image,
            reference: ResourceReference::RelativePath {
                value: "image.png".into(),
            },
        };

        router
            .accept(&encode(&Envelope::new(Message::ResourceRequest(request.clone()))).unwrap())
            .unwrap();
        assert_eq!(router.drain(), vec![BridgeMessage::Resource(request)]);
    }

    #[test]
    fn accepts_current_position_capture() {
        let context = BridgeContext {
            document: DocumentId::new(4),
            generation: Generation::new(7),
        };
        let position = PositionCaptured {
            request: id(3),
            document: context.document.unwrap(),
            generation: context.generation.unwrap(),
            locator: Locator {
                heading: Some("intro".into()),
                block: "p-3".into(),
                offset: 12,
                fallback: LocatorFallback::NearestHeading,
            },
        };
        let mut router = BridgeRouter::new(context);
        router
            .accept(&encode(&Envelope::new(Message::PositionCaptured(position.clone()))).unwrap())
            .unwrap();
        assert_eq!(
            router.drain(),
            vec![BridgeMessage::PositionCaptured(position)]
        );
    }

    #[test]
    fn rejects_malformed_unknown_and_non_action_messages() {
        let mut router = BridgeRouter::new(BridgeContext::default());
        assert!(matches!(router.accept(b"{"), Err(BridgeError::Contract(_))));
        assert!(matches!(
            router.accept(br#"{"revision":1,"message":{"kind":"unknown","payload":{}}}"#),
            Err(BridgeError::Contract(_))
        ));
        assert!(matches!(
            router.accept(br#"{"revision":1,"message":{"kind":"action","payload":{"request":1,"document":null,"generation":null,"action":{"kind":"search","payload":{"kind":"open","query":"","case_sensitive":false}}}}}"#),
            Err(BridgeError::Contract(_))
        ));
        assert!(matches!(
            router.accept(
                &encode(&Envelope::new(Message::Progress(
                    crate::contracts::ProgressMessage {
                        operation: crate::contracts::ProgressOperation::Render,
                        request: None,
                        document: None,
                        generation: None,
                        completed: 0,
                        total: None,
                    },
                )))
                .unwrap()
            ),
            Err(BridgeError::NotAction)
        ));
        assert!(router.drain_actions().is_empty());
    }

    #[test]
    fn rejects_stale_context() {
        let current = BridgeContext {
            document: Some(DocumentId::new(7).unwrap()),
            generation: Some(Generation::new(3).unwrap()),
        };
        let mut router = BridgeRouter::new(current);
        let stale = ActionMessageEnvelope {
            request: id(1),
            document: Some(DocumentId::new(6).unwrap()),
            generation: Some(Generation::new(3).unwrap()),
            action: ActionMessage::Focus(FocusOwner::Renderer),
        };

        let error = router
            .accept(&encode(&Envelope::new(Message::Action(stale))).unwrap())
            .unwrap_err();

        assert!(matches!(error, BridgeError::StaleContext { .. }));
        assert!(router.drain_actions().is_empty());
    }

    #[test]
    fn rejects_unsupported_action() {
        let mut router = BridgeRouter::new(BridgeContext::default());
        let unsupported = ActionMessage::CapturePosition(crate::contracts::CapturePosition {
            request: id(2),
            document: DocumentId::new(1).unwrap(),
            generation: Generation::new(1).unwrap(),
        });

        assert_eq!(
            router.accept(&bytes(1, None, unsupported)),
            Err(BridgeError::UnsupportedAction)
        );
        assert!(router.drain_actions().is_empty());
    }

    #[test]
    fn drains_accepted_actions_fifo() {
        let mut router = BridgeRouter::new(BridgeContext::default());
        router
            .accept(&bytes(1, None, ActionMessage::Search(SearchAction::Next)))
            .unwrap();
        router
            .accept(&bytes(2, None, ActionMessage::Copy(CopyAction::Code)))
            .unwrap();
        router
            .accept(&bytes(3, None, ActionMessage::SelectAll))
            .unwrap();

        let requests = router
            .drain()
            .into_iter()
            .map(|message| match message {
                BridgeMessage::Action(action) => action.request.get(),
                BridgeMessage::Navigation(request) => request.request.get(),
                BridgeMessage::Resource(request) => request.request.get(),
                BridgeMessage::PositionCaptured(position) => position.request.get(),
                BridgeMessage::RenderReady(_) | BridgeMessage::RenderError(_) => 0,
            })
            .collect::<Vec<_>>();
        assert_eq!(requests, [1, 2, 3]);
        assert!(router.drain_actions().is_empty());
    }

    #[test]
    fn routers_isolate_messages_between_windows() {
        let mut first = BridgeRouter::new(BridgeContext::default());
        let mut second = BridgeRouter::new(BridgeContext::default());
        first
            .accept(&bytes(1, None, ActionMessage::Search(SearchAction::Next)))
            .unwrap();
        second
            .accept(&bytes(2, None, ActionMessage::Copy(CopyAction::Code)))
            .unwrap();

        assert_eq!(first.drain_actions()[0].request.get(), 1);
        assert_eq!(second.drain_actions()[0].request.get(), 2);
        assert!(first.drain_actions().is_empty());
        assert!(second.drain_actions().is_empty());
    }

    #[test]
    fn context_changes_do_not_authorize_queued_actions() {
        let current = BridgeContext {
            document: Some(DocumentId::new(7).unwrap()),
            generation: Some(Generation::new(3).unwrap()),
        };
        let mut router = BridgeRouter::new(current);
        router
            .accept(&bytes_with_generation(
                1,
                current.document,
                current.generation,
                ActionMessage::SelectAll,
            ))
            .unwrap();

        let next = BridgeContext {
            document: Some(DocumentId::new(8).unwrap()),
            generation: Some(Generation::new(4).unwrap()),
        };
        router.set_context(next);

        assert!(router.drain_actions().is_empty());
        assert_eq!(
            router.accept(&bytes_with_generation(
                2,
                current.document,
                current.generation,
                ActionMessage::SelectAll,
            )),
            Err(BridgeError::StaleContext {
                expected: next,
                actual: current,
            })
        );
    }
}
