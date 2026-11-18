//! Diagrams written as text: Mermaid's (the ```` ```mermaid ```` blocks of a Markdown
//! document) parsed into typed models — what each diagram says, never where it goes.
//! Every error names its line (1-based, in the block).

pub mod mermaid;
