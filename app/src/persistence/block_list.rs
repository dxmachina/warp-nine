//! Manages how we write to and read from our SQLite database for terminal blocks.

use diesel::prelude::*;
use diesel::result::Error;
use diesel::sqlite::SqliteConnection;

use super::{model, schema};
use crate::terminal::model::block::{SerializedAgentViewVisibility, SerializedBlock};

const MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION: i64 = 100;

// LOCAL FORK: the AIQuery row structs and their read limits went with the
// agent; nothing writes or reads the `ai_queries` table any more.

// LOCAL FORK: fn get_all_restored_blocks removed with the agent; its result
// type came from the agent block list and nothing reads it any more.

pub(super) fn save_block(
    conn: &mut SqliteConnection,
    pane_id: Vec<u8>,
    block: &SerializedBlock,
    is_local_block: bool,
) -> Result<(), Error> {
    use schema::blocks::dsl::*;
    conn.transaction::<_, Error, _>(|conn| {
        let saved_blocks_count: i64 = schema::blocks::dsl::blocks
            .filter(pane_leaf_uuid.eq(pane_id.clone()))
            .filter(id.is_not_null())
            .filter(is_background.ne(true))
            .count()
            .first(conn)?;

        // add 1 because we are about to save a new block
        let diff = saved_blocks_count - MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION + 1;
        if diff > 0 {
            // Find the oldest block to keep.
            let last_kept_id: Option<i32> = schema::blocks::dsl::blocks
                .filter(pane_leaf_uuid.eq(pane_id.clone()))
                .filter(id.is_not_null())
                .filter(is_background.ne(true))
                .select(id)
                .order(id.asc())
                .offset(diff)
                .limit(1)
                .first(conn)?;

            if let Some(last_kept_id) = last_kept_id {
                diesel::delete(
                    schema::blocks::dsl::blocks
                        .filter(id.lt(last_kept_id))
                        .filter(pane_leaf_uuid.eq(pane_id.clone())),
                )
                .execute(conn)?;
            }
        }

        let block = create_block(pane_id, block, is_local_block);
        diesel::insert_into(schema::blocks::dsl::blocks)
            .values(block)
            .execute(conn)?;
        Ok(())
    })
}

// TODO(vorporeal): can move this to a `to_persisted_block()` function on `SerializedBlock`
// to get it out of the persistence layer.
fn create_block<'a>(
    pane_leaf_uuid: Vec<u8>,
    block: &'a SerializedBlock,
    is_local: bool,
) -> model::NewBlock<'a> {
    model::NewBlock {
        block_id: block.id.as_str(),
        pane_leaf_uuid,
        stylized_command: &block.stylized_command,
        stylized_output: &block.stylized_output,
        pwd: block.pwd.as_ref(),
        // This sqlite column still uses the legacy `git_branch` name, but it now stores the
        // block's git head for backwards compatibility with existing persisted data.
        git_branch: block.git_head.as_ref(),
        git_branch_name: block.git_branch_name.as_ref(),
        virtual_env: block.virtual_env.as_ref(),
        conda_env: block.conda_env.as_ref(),
        exit_code: block.exit_code.value(),
        did_execute: block.did_execute,
        completed_ts: block.completed_ts.map(|ts| ts.naive_utc()),
        start_ts: block.start_ts.map(|ts| ts.naive_utc()),
        ps1: block.ps1.as_ref(),
        rprompt: block.rprompt.as_ref(),
        honor_ps1: block.honor_ps1,
        is_background: block.is_background,
        shell: block.shell_host.as_ref().map(|host| host.shell_type.name()),
        user: block.shell_host.as_ref().map(|host| host.user.as_str()),
        host: block.shell_host.as_ref().map(|host| host.hostname.as_str()),
        prompt_snapshot: block.prompt_snapshot.as_ref(),
        ai_metadata: block.ai_metadata.as_ref(),
        is_local: Some(is_local),
        agent_view_visibility: block
            .agent_view_visibility
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok()),
    }
}

pub(super) fn delete_blocks(conn: &mut SqliteConnection, pane_id: Vec<u8>) -> Result<(), Error> {
    use schema::blocks::dsl::*;
    conn.transaction::<_, Error, _>(|conn| {
        diesel::delete(schema::blocks::dsl::blocks.filter(pane_leaf_uuid.eq(pane_id.clone())))
            .execute(conn)?;
        Ok(())
    })
}

pub(super) fn update_block_agent_view_visibility(
    conn: &mut SqliteConnection,
    target_block_id: &str,
    visibility: &SerializedAgentViewVisibility,
) -> anyhow::Result<()> {
    use schema::blocks::dsl::*;
    let visibility_json = serde_json::to_string(visibility)?;
    diesel::update(blocks.filter(block_id.eq(target_block_id)))
        .set(agent_view_visibility.eq(visibility_json))
        .execute(conn)?;
    Ok(())
}

pub(super) fn delete_ai_conversation(
    conn: &mut SqliteConnection,
    conversation_id_str: &str,
) -> anyhow::Result<()> {
    use schema::ai_queries::dsl as queries_dsl;

    conn.transaction::<_, Error, _>(|conn| {
        // Delete the AI query
        diesel::delete(
            queries_dsl::ai_queries.filter(queries_dsl::conversation_id.eq(conversation_id_str)),
        )
        .execute(conn)?;

        Ok(())
    })?;

    Ok(())
}

#[cfg(test)]
#[path = "block_list_tests.rs"]
mod tests;
