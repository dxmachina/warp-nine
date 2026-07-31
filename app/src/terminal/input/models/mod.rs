mod data_source;
// LOCAL FORK: `model_spec_scores` (341 lines) scored LLM specs to rank models in the
// agent's model picker. Its only reference repo-wide was this `mod` line.
mod view;

pub use data_source::{
    AcceptModel, ModelPickerChoice, ModelSelectorDataSource, query_model_picker_choices,
};
pub use view::{InlineModelSelectorEvent, InlineModelSelectorTab, InlineModelSelectorView};
