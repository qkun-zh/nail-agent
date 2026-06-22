use std::collections::VecDeque;
use std::sync::Arc;

use agent_client_protocol::{
    Client, ConnectionTo, Result as AcpResult,
    schema::{PromptRequest, PromptResponse, StopReason},
};

use crate::AppState;
use crate::fsm::converter::transit;
use crate::fsm::event::Event;
use crate::fsm::executor::{ExecutorContext, execute_effect};
use crate::fsm::state::State;

const MAX_ITERATIONS: usize = 32;

pub async fn run(
    req: PromptRequest,
    connection: ConnectionTo<Client>,
    state: Arc<AppState>,
) -> AcpResult<PromptResponse> {
    let fsm_timer = crate::logger::Timer::start("FSM.run");

    log::info!("========================================");
    log::info!("[FSM] ====== FSM started ======");
    log::info!("========================================");

    crate::logger::log_json("PromptRequest", &req);

    let content_blocks = req.prompt.clone();
    let session_id = req.session_id.clone();
    let user_text: String = content_blocks
        .iter()
        .filter_map(|block| {
            if let agent_client_protocol::schema::ContentBlock::Text(text) = block {
                Some(text.text.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    log::info!(
        "[FSM] Prompt received: session_id={}, text_len={}, blocks={}",
        session_id,
        user_text.len(),
        content_blocks.len()
    );
    log::debug!(
        "[FSM] Prompt preview: {:?}",
        &user_text[..std::cmp::min(user_text.len(), 500)]
    );

    if user_text.is_empty() {
        log::warn!("[FSM] Empty prompt, returning directly");
        let resp = PromptResponse::new(StopReason::EndTurn);
        crate::logger::log_json("PromptResponse", &resp);
        return Ok(resp);
    }

    if let Err(e) = crate::db::append_message(&session_id.to_string(), "user", &user_text).await {
        log::error!("[FSM] Failed to save user message: {}", e);
    }

    let exec_ctx = ExecutorContext {
        llm: &state.llm,
        model_name: &state.model_name,
        proxy_handle: &state.proxy_handle,
        connection: &connection,
    };

    let mut current_state = State::Idle {
        session_id: session_id.clone(),
    };
    let mut event_queue: VecDeque<Event> = VecDeque::new();
    event_queue.push_back(Event::UserMessageArrived);

    let mut iteration_count = 0usize;
    let loop_start = std::time::Instant::now();

    let result = loop {
        let elapsed_so_far = loop_start.elapsed().as_millis();

        let Some(current_event) = event_queue.pop_front() else {
            log::info!("[FSM] Event queue empty, FSM finished");
            break PromptResponse::new(StopReason::EndTurn);
        };

        log::info!(
            "[FSM] ======== Iteration (State: {:?}, elapsed: {}ms) ========",
            state_name(&current_state),
            elapsed_so_far
        );

        if elapsed_so_far > 60_000 {
            log::warn!(
                "[FSM] FSM running over 60s! ({}ms), current state: {:?}",
                elapsed_so_far,
                state_name(&current_state)
            );
        }
        if elapsed_so_far > 120_000 {
            log::error!("[FSM] FSM running over 120s! Force terminating");
            break PromptResponse::new(StopReason::EndTurn);
        }

        log::info!(
            "[FSM] Transition: {:?} --[{:?}]--> (Effect)",
            state_name(&current_state),
            event_name(&current_event),
        );

        let (next_state, effect) = match transit(current_state.clone(), current_event) {
            Ok(t) => t,
            Err(e) => {
                log::error!("[FSM] Transition failed: {}", e);
                break PromptResponse::new(StopReason::EndTurn);
            }
        };

        log::info!(
            "[FSM]   -> Transition result: {:?} --[Event consumed]--> {:?} (Effect: {:?})",
            state_name(&current_state),
            state_name(&next_state),
            effect_name(&effect)
        );

        if matches!(next_state, State::Done) {
            if !matches!(effect, crate::fsm::effect::Effect::DoNothing) {
                log::info!("[FSM] Done state: executing final effect");
                let _ = execute_effect(effect, &exec_ctx).await;
            }
            log::info!("[FSM] FSM finished, total iterations: {}", iteration_count);
            break PromptResponse::new(StopReason::EndTurn);
        }

        // Step 3: Check iteration limit (counted on CallingTools -> CallingLlm)
        if matches!(&next_state, State::ModelCalling { .. })
            && matches!(&current_state, State::ToolsCalling { .. })
        {
            iteration_count += 1;
            log::info!(
                "[FSM] LLM call #{} (limit: {})",
                iteration_count,
                MAX_ITERATIONS
            );
            if iteration_count > MAX_ITERATIONS {
                log::error!(
                    "[FSM] Exceeded max iterations ({}), terminating",
                    MAX_ITERATIONS
                );
                break PromptResponse::new(StopReason::EndTurn);
            }
        }

        current_state = next_state;

        // Step 4: Execute effect (async IO)
        if !matches!(effect, crate::fsm::effect::Effect::DoNothing) {
            let exec_timer =
                crate::logger::Timer::start(format!("execute_effect({:?})", effect_name(&effect)));
            let new_events = execute_effect(effect, &exec_ctx).await.map_err(|e| {
                log::error!("[FSM] Failed to execute effect: {}", e);
                agent_client_protocol::Error::internal_error()
            })?;
            exec_timer.stop();

            log::info!(
                "[FSM] Effect complete, produced {} events",
                new_events.len()
            );
            event_queue.extend(new_events);
        }
    };

    fsm_timer.stop();
    log::info!("========================================");
    log::info!("[FSM] ====== FSM finished ======");
    log::info!("========================================");

    crate::logger::log_json("PromptResponse", &result);
    Ok(result)
}

fn state_name(s: &State) -> &'static str {
    match s {
        State::Idle { .. } => "Idle",
        State::ContextLoading { .. } => "Preparing",
        State::ModelCalling { .. } => "CallingLlm",
        State::ModelResponseStreaming { .. } => "Streaming",
        State::ToolsCalling { .. } => "CallingTools",
        State::Done => "Done",
    }
}

fn event_name(e: &Event) -> &'static str {
    match e {
        Event::UserMessageArrived { .. } => "UserPromptArrived",
        Event::ContextLoaded { .. } => "ContextLoaded",
        Event::FirstChunkOfModelResponseArrived { .. } => "FirstDeltaArrived",
        Event::NextChunkOfModelResponseArrived { .. } => "NextDeltaArrived",
        Event::ModelResponseFinishedWithoutToolCalls { .. } => "LlmStreamFinishedWithoutToolCalls",
        Event::ModelResponseFinishedWithToolCalls { .. } => "LlmStreamFinishedWithToolCalls",
        Event::ToolsResponseArrived { .. } => "ToolResultArrived",
    }
}

fn effect_name(e: &crate::fsm::effect::Effect) -> &'static str {
    match e {
        crate::fsm::effect::Effect::LoadContext { .. } => "LoadContext",
        crate::fsm::effect::Effect::CallModel { .. } => "CallLlmStream",
        crate::fsm::effect::Effect::CallTools { .. } => "CallTools",
        crate::fsm::effect::Effect::SaveSession { .. } => "SaveSession",
        crate::fsm::effect::Effect::DoNothing => "None",
    }
}
