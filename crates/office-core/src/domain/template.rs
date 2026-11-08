//! Pure template rules, bundled Markdown and checks shared by knowledge consumers.

mod bundled;
mod rules;
mod sections;
mod types;

pub(crate) use bundled::frontmatter_scalar;
pub use bundled::{
    BUNDLED, BundledTemplate, KnowledgePage, TemplateAssessment, TemplateFile, assess_templates,
};
pub use types::{
    AttachPolicy, IdPattern, Performable, Submission, SubmissionVia, Template, TemplateKind,
};
