use crate::fsm::effect::Effect;
use crate::fsm::event::Event;
use crate::fsm::state::State;
use anyhow::{Result, anyhow};

pub fn transit(state: State, event: Event) -> Result<(State, Effect)> {
    match (state, event) {
        (State::Idle { session_id }, Event::UserMessageArrived { .. }) => Ok((
            State::ContextLoading {
                session_id: session_id.clone(),
            },
            Effect::LoadContext { session_id },
        )),

        (
            State::ContextLoading { session_id },
            Event::ContextLoaded {
                messages, tools, ..
            },
        ) => {
            let next_tools = tools.clone();
            Ok((
                State::ModelCalling {
                    session_id: session_id.clone(),
                    messages: messages.clone(),
                    tools: next_tools.clone(),
                },
                Effect::CallModel {
                    messages,
                    tools: next_tools,
                    session_id,
                },
            ))
        }

        (
            State::ModelCalling {
                session_id,
                messages,
                tools,
            },
            Event::FirstChunkOfModelResponseArrived { delta, .. },
        ) => Ok((
            State::ModelResponseStreaming {
                session_id,
                messages,
                tools,
                accumulated: delta,
            },
            Effect::DoNothing,
        )),

        (
            State::ModelResponseStreaming {
                session_id,
                messages,
                tools,
                accumulated,
            },
            Event::NextChunkOfModelResponseArrived { delta, .. },
        ) => Ok((
            State::ModelResponseStreaming {
                session_id,
                messages,
                tools,
                accumulated: accumulated + &delta,
            },
            Effect::DoNothing,
        )),

        (
            State::ModelResponseStreaming { session_id, .. },
            Event::ModelResponseFinishedWithoutToolCalls { full_content, .. },
        ) => Ok((
            State::Done,
            Effect::SaveSession {
                content: full_content,
                session_id,
            },
        )),

        (
            State::ModelResponseStreaming {
                session_id,
                messages,
                tools,
                ..
            },
            Event::ModelResponseFinishedWithToolCalls { tool_calls, .. },
        ) => Ok((
            State::ToolsCalling {
                session_id: session_id.clone(),
                tools: tools.clone(),
            },
            Effect::CallTools {
                messages,
                tool_calls,
                session_id,
            },
        )),

        (
            State::ToolsCalling {
                session_id, tools, ..
            },
            Event::ToolsResponseArrived {
                updated_messages, ..
            },
        ) => {
            let next_tools = tools.clone();
            Ok((
                State::ModelCalling {
                    session_id: session_id.clone(),
                    messages: updated_messages.clone(),
                    tools: next_tools.clone(),
                },
                Effect::CallModel {
                    messages: updated_messages,
                    tools: next_tools,
                    session_id,
                },
            ))
        }

        _ => Err(anyhow!("invalid state transition")),
    }
}
