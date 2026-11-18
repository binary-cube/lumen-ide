//! HTTP requests as the user writes them (plan 77): `.http` files in the syntax other HTTP
//! clients write too ([`parse`] into the [`model`]), their environments beside them
//! ([`env`]: public, private, local), the variables resolved ([`vars`]), cURL in and out
//! ([`curl`]), other clients' collections in ([`import`]), a project's cookies ([`cookies`]).
//! No network, no UI: the transport is a port ([`transport`]) its adapter implements, the modules only wire.

pub mod chain;
pub mod cookies;
pub mod curl;
pub mod digest;
pub mod draft;
pub mod env;
pub mod env_edit;
pub mod faker;
pub mod hash;
pub mod import;
pub mod jsonpath;
pub mod model;
pub mod oauth;
pub mod openapi;
pub mod parse;
pub mod prepare;
pub mod script;
pub mod transport;
pub mod url;
pub mod vars;
pub mod xpath;

pub use model::{BodyPart, FileVariable, Header, HttpFile, Options, Request, SaveTo, Script};
pub use parse::parse;
