//! A conversation as the editor shows it, built from the agent's updates (`session/update`)
//! and what was typed: messages (the user's, the agent's, its thinking), the tools it uses
//! (each updated in place: its status, its output, its changes as diffs), its plan, the
//! permission it waits for. Pure: given the updates, the same items; every change counted
//! ([`Conversation::version`]), and each item stamped with the change that last touched it
//! ([`Conversation::revision`]), so a view lays out again only what changed without reading
//! what the others hold. A diff's lines added and removed are counted once, as it arrives.

use std::collections::HashSet;

use serde_json::Value;

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Author {
    User,
    Agent,
    /// Its reasoning, shown quieter.
    Thought,
}

/// A tool's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolStatus {
    Pending,
    Running,
    Done,
    Failed,
}

impl ToolStatus {
    fn read(status: &str) -> Self {
        match status {
            "in_progress" => Self::Running,
            "completed" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A change a tool makes to a file: its text before (`None`: a new file) and after, the
/// lines it adds and removes (counted once, by [`Diff::new`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Diff {
    pub path: String,
    pub old: Option<String>,
    pub new: String,
    pub added: usize,
    pub removed: usize,
}

impl Diff {
    /// The change of `path` from `old` to `new`, its lines counted.
    pub fn new(path: String, old: Option<String>, new: String) -> Self {
        let (added, removed) = counts(old.as_deref(), &new);
        Self { path, old, new, added, removed }
    }
}

/// The lines a change adds and removes, as a set's difference (cheap, enough for a sum): the
/// new lines found nowhere in the old text, the old ones found nowhere in the new, each
/// counted as often as it occurs. Linear: each side's lines looked up in a set of the other's.
pub fn counts(old: Option<&str>, new: &str) -> (usize, usize) {
    let old: Vec<&str> = old.map(|o| o.lines().collect()).unwrap_or_default();
    let new: Vec<&str> = new.lines().collect();
    let in_old: HashSet<&str> = old.iter().copied().collect();
    let in_new: HashSet<&str> = new.iter().copied().collect();
    let added = new.iter().filter(|l| !in_old.contains(*l)).count();
    let removed = old.iter().filter(|l| !in_new.contains(*l)).count();
    (added, removed)
}

/// A tool the agent uses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tool {
    pub id: String,
    pub title: String,
    /// `read`, `edit`, `execute`, `search`… (the agent's word).
    pub kind: String,
    pub status: ToolStatus,
    /// What it said (its text output).
    pub output: String,
    pub diffs: Vec<Diff>,
    /// The files it's about.
    pub paths: Vec<String>,
}

/// A step of the agent's plan.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Step {
    pub text: String,
    pub done: bool,
    pub running: bool,
}

/// What the conversation shows, in order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Item {
    Message { author: Author, text: String },
    Tool(Tool),
    Plan(Vec<Step>),
}

/// A choice the agent offers for a permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always`.
    pub kind: String,
}

/// A permission the agent waits for: the request's id (answered with it), the tool it's
/// for, the choices.
#[derive(Debug, Clone, PartialEq)]
pub struct Permission {
    pub request: Value,
    pub tool: Option<String>,
    pub title: String,
    pub choices: Vec<Choice>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Conversation {
    pub items: Vec<Item>,
    pub permission: Option<Permission>,
    /// A turn runs (a prompt sent, not answered yet).
    pub running: bool,
    version: u64,
    /// Each item's [`Self::revision`], by its place.
    revisions: Vec<u64>,
}

/// A content block's text (`{"type": "text", "text": …}`); other blocks say nothing here.
fn text_of(content: &Value) -> Option<&str> {
    (content.get("type")?.as_str()? == "text").then(|| content.get("text")?.as_str())?
}

impl Conversation {
    /// Counts every change (a view lays out again when it moves).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The version that last changed item `at` (`0`: no such item): the same while the item
    /// holds the same, so a view keeps what it made of it.
    pub fn revision(&self, at: usize) -> u64 {
        self.revisions.get(at).copied().unwrap_or(0)
    }

    /// A change counted, item `at` stamped with it.
    fn touched(&mut self, at: usize) {
        self.version += 1;
        self.revisions.resize(self.items.len(), 0);
        if let Some(revision) = self.revisions.get_mut(at) {
            *revision = self.version;
        }
    }

    /// `text`, typed by the user and sent.
    pub fn typed(&mut self, text: &str) {
        self.items.push(Item::Message { author: Author::User, text: text.to_owned() });
        self.running = true;
        self.touched(self.items.len() - 1);
    }

    /// The turn ended (its reason: `end_turn`, `cancelled`…); a permission left is let go.
    pub fn ended(&mut self) {
        self.running = false;
        self.permission = None;
        self.version += 1;
    }

    /// A chunk of a message from `author`: added to its last message when that's the
    /// latest item, else a new one. Returns the message's place.
    fn chunk(&mut self, author: Author, text: &str) -> usize {
        if let Some(Item::Message { author: last, text: kept }) = self.items.last_mut()
            && *last == author
        {
            kept.push_str(text);
        } else {
            self.items.push(Item::Message { author, text: text.to_owned() });
        }
        self.items.len() - 1
    }

    /// The tool `id`'s place, made when it's new.
    fn tool(&mut self, id: &str) -> usize {
        let at = self.items.iter().position(|i| matches!(i, Item::Tool(t) if t.id == id));
        at.unwrap_or_else(|| {
            self.items.push(Item::Tool(Tool {
                id: id.to_owned(),
                title: String::new(),
                kind: String::new(),
                status: ToolStatus::Pending,
                output: String::new(),
                diffs: Vec::new(),
                paths: Vec::new(),
            }));
            self.items.len() - 1
        })
    }

    /// A tool call's fields (new, or updated: only what's given changes). Returns the
    /// tool's place (`None`: no tool named).
    fn tool_fields(&mut self, update: &Value) -> Option<usize> {
        let id = update.get("toolCallId").and_then(Value::as_str)?;
        let at = self.tool(id);
        let Item::Tool(tool) = &mut self.items[at] else {
            unreachable!("the position found is a tool's")
        };
        let field = |key: &str| update.get(key).and_then(Value::as_str);
        if let Some(title) = field("title") {
            title.clone_into(&mut tool.title);
        }
        if let Some(kind) = field("kind") {
            kind.clone_into(&mut tool.kind);
        }
        if let Some(status) = field("status") {
            tool.status = ToolStatus::read(status);
        }
        if let Some(locations) = update.get("locations").and_then(Value::as_array) {
            tool.paths = locations
                .iter()
                .filter_map(|l| l.get("path")?.as_str().map(str::to_owned))
                .collect();
        }
        if let Some(content) = update.get("content").and_then(Value::as_array) {
            let mut output = String::new();
            let mut diffs = Vec::new();
            for item in content {
                match item.get("type").and_then(Value::as_str) {
                    Some("content") => {
                        if let Some(text) = item.get("content").and_then(text_of) {
                            output.push_str(text);
                        }
                    }
                    Some("diff") => {
                        let text = |k: &str| item.get(k).and_then(Value::as_str).map(str::to_owned);
                        if let (Some(path), Some(new)) = (text("path"), text("newText")) {
                            diffs.push(Diff::new(path, text("oldText"), new));
                        }
                    }
                    _ => {}
                }
            }
            tool.output = output;
            tool.diffs = diffs;
        }
        Some(at)
    }

    /// A `session/update`'s `update`, applied. `true` if it changed something shown.
    pub fn update(&mut self, update: &Value) -> bool {
        let kind = update.get("sessionUpdate").and_then(Value::as_str).unwrap_or_default();
        let content = update.get("content");
        let changed = match kind {
            "agent_message_chunk" | "user_message_chunk" | "agent_thought_chunk" => {
                let Some(text) = content.and_then(text_of) else { return false };
                let author = match kind {
                    "user_message_chunk" => Author::User,
                    "agent_thought_chunk" => Author::Thought,
                    _ => Author::Agent,
                };
                Some(self.chunk(author, text))
            }
            "tool_call" | "tool_call_update" => self.tool_fields(update),
            "plan" => {
                let steps = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|e| {
                                let status = e.get("status").and_then(Value::as_str);
                                Some(Step {
                                    text: e.get("content")?.as_str()?.to_owned(),
                                    done: status == Some("completed"),
                                    running: status == Some("in_progress"),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // One plan, kept current where it first showed.
                match self.items.iter().position(|i| matches!(i, Item::Plan(_))) {
                    Some(at) => {
                        self.items[at] = Item::Plan(steps);
                        Some(at)
                    }
                    None => {
                        self.items.push(Item::Plan(steps));
                        Some(self.items.len() - 1)
                    }
                }
            }
            _ => return false,
        };
        match changed {
            Some(at) => self.touched(at),
            None => self.version += 1,
        }
        true
    }

    /// The agent asks for a permission (`session/request_permission`, request `id`).
    pub fn ask(&mut self, id: Value, params: &Value) {
        let call = params.get("toolCall");
        let tool =
            call.and_then(|c| c.get("toolCallId")).and_then(Value::as_str).map(str::to_owned);
        if let Some(at) = call.and_then(|c| self.tool_fields(c)) {
            self.touched(at);
        }
        let title = tool
            .as_deref()
            .and_then(|id| {
                self.items.iter().find_map(|i| match i {
                    Item::Tool(t) if t.id == id && !t.title.is_empty() => Some(t.title.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default();
        let choices = params
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|o| {
                        let text = |k: &str| o.get(k)?.as_str().map(str::to_owned);
                        Some(Choice {
                            id: text("optionId")?,
                            name: text("name")?,
                            kind: text("kind")?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.permission = Some(Permission { request: id, tool, title, choices });
        self.version += 1;
    }

    /// The permission answered: its request, to reply to.
    pub fn answered(&mut self) -> Option<Value> {
        let permission = self.permission.take()?;
        self.version += 1;
        Some(permission.request)
    }
}

#[cfg(test)]
#[path = "../tests/unit/conversation.rs"]
mod tests;
