mod batch;
mod comment;
// LOCAL FORK: `convert` existed only to turn an agent `InsertReviewComment` action
// into a review comment, and `diff_hunk_parser` existed only to serve `convert`.
// Tracing `main`, the only producers of pending imported comments were the agent
// block handler and the agent-action path in `terminal/view.rs`, so the fork has no
// surviving source of GitHub review comments to parse hunks for.
// `flatten` went the same way: `attach_pending_imported_comments` was only ever reached
// through `terminal::view::Event::InsertCodeReviewComments`, which nothing emits any more.

pub(crate) use batch::{ReviewCommentBatch, ReviewCommentBatchEvent};
#[cfg(test)]
pub(crate) use comment::ImportedCommentDetails;
pub(crate) use comment::{
    AttachedReviewComment, AttachedReviewCommentTarget, CommentId, CommentOrigin, LineDiffContent,
};
