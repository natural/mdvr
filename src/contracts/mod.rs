//! M1 bridge contract. JSON is the only wire format; native policy remains the
//! authority for paths, URLs, permissions, and transport.

use std::{fmt, num::NonZeroU64};

use serde::{Deserialize, Serialize};

pub const CONTRACT_REVISION: u16 = 1;
pub const MAX_TEXT_BYTES: usize = 4 * 1024;
pub const MAX_PATH_BYTES: usize = 4 * 1024;
pub const MAX_SOURCE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_RESOURCE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FRAME_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_BATCH_ITEMS: usize = 256;
pub const MAX_HEADINGS: usize = 4096;
pub const MAX_SYNTAX_TOKENS: usize = 128;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Option<Self> {
                NonZeroU64::new(value).map(Self)
            }

            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

id_type!(InvocationId);
id_type!(RootId);
id_type!(ScanId);
id_type!(DocumentId);
id_type!(Generation);
id_type!(RequestId);
id_type!(ResourceId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    Json(String),
    UnknownRevision(u16),
    FrameTooLarge {
        bytes: usize,
        max: usize,
    },
    EmptyField(&'static str),
    TextTooLong {
        field: &'static str,
        bytes: usize,
        max: usize,
    },
    InvalidPath(String),
    InvalidColor(String),
    InvalidScale(u16),
    TooMany {
        field: &'static str,
        count: usize,
        max: usize,
    },
    ZeroId(&'static str),
    StaleGeneration {
        expected: Generation,
        actual: Generation,
    },
    StaleScan {
        expected: ScanId,
        actual: ScanId,
    },
    MismatchedDocument {
        expected: DocumentId,
        actual: DocumentId,
    },
    InvalidResourceKind,
    InvalidPayload(&'static str),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ContractError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub revision: u16,
    pub message: Message,
}

impl Envelope {
    pub fn new(message: Message) -> Self {
        Self {
            revision: CONTRACT_REVISION,
            message,
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.revision != CONTRACT_REVISION {
            return Err(ContractError::UnknownRevision(self.revision));
        }
        self.message.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum Message {
    #[serde(rename = "launch.request")]
    LaunchRequest(LaunchRequest),
    #[serde(rename = "launch.ack")]
    LaunchAck(LaunchAck),
    #[serde(rename = "discovery.request")]
    DiscoveryRequest(DiscoveryRequest),
    #[serde(rename = "discovery.batch")]
    DiscoveryBatch(DiscoveryBatch),
    #[serde(rename = "discovery.complete")]
    DiscoveryComplete(DiscoveryComplete),
    #[serde(rename = "discovery.error")]
    DiscoveryError(DiscoveryError),
    #[serde(rename = "document.load")]
    DocumentLoad(DocumentLoad),
    #[serde(rename = "render.ready")]
    RenderReady(RenderReady),
    #[serde(rename = "render.error")]
    RenderError(RenderError),
    #[serde(rename = "position.captured")]
    PositionCaptured(PositionCaptured),
    #[serde(rename = "position.restored")]
    PositionRestored(PositionRestored),
    #[serde(rename = "navigation.request")]
    NavigationRequest(NavigationRequest),
    #[serde(rename = "navigation.result")]
    NavigationResult(NavigationResult),
    #[serde(rename = "resource.request")]
    ResourceRequest(ResourceRequest),
    #[serde(rename = "resource.result")]
    ResourceResult(ResourceResult),
    #[serde(rename = "resource.revoked")]
    ResourceRevoked(ResourceRevoked),
    #[serde(rename = "appearance.update")]
    AppearanceUpdate(AppearanceUpdate),
    #[serde(rename = "action")]
    Action(ActionMessageEnvelope),
    #[serde(rename = "error")]
    Error(ErrorMessage),
    #[serde(rename = "progress")]
    Progress(ProgressMessage),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchIntent {
    Picker,
    File,
    Restore,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchRequest {
    pub invocation: InvocationId,
    pub intent: LaunchIntent,
    pub caller_cwd: String,
    pub path: Option<String>,
    pub ack_timeout_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchOutcome {
    Accepted,
    Failed,
    TimedOut,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchAck {
    pub invocation: InvocationId,
    pub outcome: LaunchOutcome,
    pub error: Option<ErrorCode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidPayload,
    MissingPath,
    UnreadablePath,
    Stale,
    Denied,
    NotFound,
    Unsupported,
    Timeout,
    Parse,
    Render,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryRequest {
    pub root: RootId,
    pub root_path: String,
    pub scan: ScanId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryEntry {
    pub relative_path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryBatch {
    pub root: RootId,
    pub scan: ScanId,
    pub entries: Vec<DiscoveryEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryComplete {
    pub root: RootId,
    pub scan: ScanId,
    pub matched: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryError {
    pub root: RootId,
    pub scan: ScanId,
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocatorFallback {
    NearestHeading,
    DocumentStart,
    DocumentEnd,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locator {
    pub heading: Option<String>,
    pub block: String,
    pub offset: u32,
    pub fallback: LocatorFallback,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentLoad {
    pub document: DocumentId,
    pub generation: Generation,
    pub path: String,
    pub source: String,
    pub anchor: Option<String>,
    pub locator: Option<Locator>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heading {
    pub id: String,
    pub text: String,
    pub level: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderReady {
    pub document: DocumentId,
    pub generation: Generation,
    pub headings: Vec<Heading>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderError {
    pub document: DocumentId,
    pub generation: Generation,
    pub code: ErrorCode,
    pub message: String,
    pub retry: Option<RetryAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionCaptured {
    pub request: RequestId,
    pub document: DocumentId,
    pub generation: Generation,
    pub locator: Locator,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionRestored {
    pub request: RequestId,
    pub document: DocumentId,
    pub generation: Generation,
    pub restored: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationTarget {
    Anchor { value: String },
    Markdown { path: String },
    Http { url: String },
    Mailto { url: String },
    LocalFile { path: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationRequest {
    pub request: RequestId,
    pub document: DocumentId,
    pub generation: Generation,
    pub target: NavigationTarget,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationDecision {
    Accepted,
    Rejected,
    NeedsConfirmation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationAuthority {
    NativePolicy,
    NativeConfirmation,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationResult {
    pub request: RequestId,
    pub document: DocumentId,
    pub generation: Generation,
    pub decision: NavigationDecision,
    pub authority: NavigationAuthority,
    pub error: Option<ErrorCode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Image,
    SvgReference,
    MermaidAsset,
    MathAsset,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceReference {
    RelativePath { value: String },
    RemoteUrl { value: String },
    Opaque { value: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRequest {
    pub request: RequestId,
    pub resource: ResourceId,
    pub document: DocumentId,
    pub generation: Generation,
    pub kind: ResourceKind,
    pub reference: ResourceReference,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceResult {
    pub request: RequestId,
    pub resource: ResourceId,
    pub document: DocumentId,
    pub generation: Generation,
    pub result: ResourceResultValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceResultValue {
    Bytes { mime: String, bytes: Vec<u8> },
    Denied { code: ErrorCode },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRevoked {
    pub document: DocumentId,
    pub generation: Generation,
    pub resource: ResourceId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    Light,
    Dark,
    System,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxRole {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Operator,
    Punctuation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntaxToken {
    pub role: SyntaxRole,
    pub foreground: String,
    pub background: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    pub mode: AppearanceMode,
    pub scale_percent: u16,
    pub reader_background: String,
    pub reader_foreground: String,
    pub code_background: String,
    pub accent: String,
    pub syntax: Vec<SyntaxToken>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceUpdate {
    pub document: Option<DocumentId>,
    pub appearance: Appearance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum ActionMessage {
    #[serde(rename = "search")]
    Search(SearchAction),
    #[serde(rename = "copy")]
    Copy(CopyAction),
    #[serde(rename = "select_all")]
    SelectAll,
    #[serde(rename = "outline")]
    Outline(OutlineAction),
    #[serde(rename = "capture_position")]
    CapturePosition(CapturePosition),
    #[serde(rename = "restore_position")]
    RestorePosition(RestorePosition),
    #[serde(rename = "focus")]
    Focus(FocusOwner),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchAction {
    Open { query: String, case_sensitive: bool },
    Next,
    Previous,
    Close,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyAction {
    Source,
    Code,
    Rendered,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutlineAction {
    Open,
    Close,
    Navigate { heading: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusOwner {
    Shell,
    Picker,
    Search,
    Palette,
    Renderer,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionMessageEnvelope {
    pub request: RequestId,
    pub document: Option<DocumentId>,
    pub generation: Option<Generation>,
    pub action: ActionMessage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePosition {
    pub request: RequestId,
    pub document: DocumentId,
    pub generation: Generation,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestorePosition {
    pub request: RequestId,
    pub document: DocumentId,
    pub generation: Generation,
    pub locator: Locator,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryAction {
    Retry,
    ChooseFile,
    ChooseFolder,
    Reload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorScope {
    Launch,
    Discovery,
    Document,
    Render,
    Navigation,
    Resource,
    Appearance,
    Action,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorMessage {
    pub scope: ErrorScope,
    pub code: ErrorCode,
    pub message: String,
    pub retry: Option<RetryAction>,
    pub document: Option<DocumentId>,
    pub generation: Option<Generation>,
    pub scan: Option<ScanId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressOperation {
    Discovery,
    Load,
    Render,
    Resource,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressMessage {
    pub operation: ProgressOperation,
    pub request: Option<RequestId>,
    pub document: Option<DocumentId>,
    pub generation: Option<Generation>,
    pub completed: u64,
    pub total: Option<u64>,
}

pub fn encode(envelope: &Envelope) -> Result<Vec<u8>, ContractError> {
    envelope.validate()?;
    let bytes =
        serde_json::to_vec(envelope).map_err(|error| ContractError::Json(error.to_string()))?;
    check_frame(bytes.len())?;
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Envelope, ContractError> {
    check_frame(bytes.len())?;
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|error| ContractError::Json(error.to_string()))?;
    envelope.validate()?;
    Ok(envelope)
}

pub fn canonical_json(envelope: &Envelope) -> Result<String, ContractError> {
    Ok(String::from_utf8(encode(envelope)?).expect("serde_json emits UTF-8"))
}

pub fn reject_stale_generation(
    expected: Generation,
    actual: Generation,
) -> Result<(), ContractError> {
    (expected == actual)
        .then_some(())
        .ok_or(ContractError::StaleGeneration { expected, actual })
}

pub fn reject_stale_scan(expected: ScanId, actual: ScanId) -> Result<(), ContractError> {
    (expected == actual)
        .then_some(())
        .ok_or(ContractError::StaleScan { expected, actual })
}

impl DiscoveryBatch {
    pub fn validate_for(&self, root: RootId, scan: ScanId) -> Result<(), ContractError> {
        if self.root != root {
            return Err(ContractError::InvalidPayload("discovery root changed"));
        }
        reject_stale_scan(scan, self.scan)?;
        self.validate()
    }
}

impl DiscoveryComplete {
    pub fn validate_for(&self, root: RootId, scan: ScanId) -> Result<(), ContractError> {
        if self.root != root {
            return Err(ContractError::InvalidPayload("discovery root changed"));
        }
        reject_stale_scan(scan, self.scan)
    }
}

impl DiscoveryError {
    pub fn validate_for(&self, root: RootId, scan: ScanId) -> Result<(), ContractError> {
        if self.root != root {
            return Err(ContractError::InvalidPayload("discovery root changed"));
        }
        reject_stale_scan(scan, self.scan)?;
        self.validate()
    }
}

impl RenderReady {
    pub fn validate_for(
        &self,
        document: DocumentId,
        generation: Generation,
    ) -> Result<(), ContractError> {
        if self.document != document {
            return Err(ContractError::MismatchedDocument {
                expected: document,
                actual: self.document,
            });
        }
        reject_stale_generation(generation, self.generation)?;
        self.validate()
    }
}

impl RenderError {
    pub fn validate_for(
        &self,
        document: DocumentId,
        generation: Generation,
    ) -> Result<(), ContractError> {
        if self.document != document {
            return Err(ContractError::MismatchedDocument {
                expected: document,
                actual: self.document,
            });
        }
        reject_stale_generation(generation, self.generation)?;
        self.validate()
    }
}

impl NavigationResult {
    pub fn validate_for(
        &self,
        document: DocumentId,
        generation: Generation,
    ) -> Result<(), ContractError> {
        if self.document != document {
            return Err(ContractError::MismatchedDocument {
                expected: document,
                actual: self.document,
            });
        }
        reject_stale_generation(generation, self.generation)?;
        self.validate()
    }
}

fn check_frame(bytes: usize) -> Result<(), ContractError> {
    if bytes > MAX_FRAME_BYTES {
        Err(ContractError::FrameTooLarge {
            bytes,
            max: MAX_FRAME_BYTES,
        })
    } else {
        Ok(())
    }
}

fn id<T>(value: T, field: &'static str) -> Result<(), ContractError>
where
    T: Into<u64>,
{
    if value.into() == 0 {
        Err(ContractError::ZeroId(field))
    } else {
        Ok(())
    }
}

fn text(value: &str, field: &'static str, max: usize) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::EmptyField(field));
    }
    if value.len() > max {
        return Err(ContractError::TextTooLong {
            field,
            bytes: value.len(),
            max,
        });
    }
    Ok(())
}

fn optional_text(
    value: Option<&str>,
    field: &'static str,
    max: usize,
) -> Result<(), ContractError> {
    value.map_or(Ok(()), |value| text(value, field, max))
}

fn path(value: &str, field: &'static str) -> Result<(), ContractError> {
    text(value, field, MAX_PATH_BYTES)?;
    if !value.starts_with('/') {
        return Err(ContractError::InvalidPath(value.to_owned()));
    }
    Ok(())
}

fn color(value: &str) -> Result<(), ContractError> {
    text(value, "color", MAX_TEXT_BYTES)?;
    let valid = value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit);
    if valid {
        Ok(())
    } else {
        Err(ContractError::InvalidColor(value.to_owned()))
    }
}

fn list(count: usize, field: &'static str, max: usize) -> Result<(), ContractError> {
    if count > max {
        Err(ContractError::TooMany { field, count, max })
    } else {
        Ok(())
    }
}

fn ids(envelope: &Envelope) -> Result<(), ContractError> {
    match &envelope.message {
        Message::LaunchRequest(v) => {
            id(v.invocation.get(), "invocation")?;
        }
        Message::LaunchAck(v) => {
            id(v.invocation.get(), "invocation")?;
        }
        Message::DiscoveryRequest(v) => {
            id(v.root.get(), "root")?;
            id(v.scan.get(), "scan")?;
        }
        Message::DiscoveryBatch(v) => {
            id(v.root.get(), "root")?;
            id(v.scan.get(), "scan")?;
        }
        Message::DiscoveryComplete(v) => {
            id(v.root.get(), "root")?;
            id(v.scan.get(), "scan")?;
        }
        Message::DiscoveryError(v) => {
            id(v.root.get(), "root")?;
            id(v.scan.get(), "scan")?;
        }
        Message::DocumentLoad(v) => {
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::RenderReady(v) => {
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::RenderError(v) => {
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::PositionCaptured(v) => {
            id(v.request.get(), "request")?;
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::PositionRestored(v) => {
            id(v.request.get(), "request")?;
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::NavigationRequest(v) => {
            id(v.request.get(), "request")?;
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::NavigationResult(v) => {
            id(v.request.get(), "request")?;
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::ResourceRequest(v) => {
            id(v.request.get(), "request")?;
            id(v.resource.get(), "resource")?;
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::ResourceResult(v) => {
            id(v.request.get(), "request")?;
            id(v.resource.get(), "resource")?;
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
        }
        Message::ResourceRevoked(v) => {
            id(v.document.get(), "document")?;
            id(v.generation.get(), "generation")?;
            id(v.resource.get(), "resource")?;
        }
        Message::AppearanceUpdate(v) => {
            if let Some(document) = v.document {
                id(document.get(), "document")?;
            }
        }
        Message::Action(v) => {
            id(v.request.get(), "request")?;
            if let Some(document) = v.document {
                id(document.get(), "document")?;
            }
            if let Some(generation) = v.generation {
                id(generation.get(), "generation")?;
            }
        }
        Message::Error(v) => {
            if let Some(document) = v.document {
                id(document.get(), "document")?;
            }
            if let Some(generation) = v.generation {
                id(generation.get(), "generation")?;
            }
            if let Some(scan) = v.scan {
                id(scan.get(), "scan")?;
            }
        }
        Message::Progress(v) => {
            if let Some(request) = v.request {
                id(request.get(), "request")?;
            }
            if let Some(document) = v.document {
                id(document.get(), "document")?;
            }
            if let Some(generation) = v.generation {
                id(generation.get(), "generation")?;
            }
        }
    }
    Ok(())
}

trait Validate {
    fn validate(&self) -> Result<(), ContractError>;
}

impl Validate for Message {
    fn validate(&self) -> Result<(), ContractError> {
        let envelope = Envelope {
            revision: CONTRACT_REVISION,
            message: self.clone(),
        };
        ids(&envelope)?;
        match self {
            Self::LaunchRequest(v) => v.validate(),
            Self::LaunchAck(v) => v.validate(),
            Self::DiscoveryRequest(v) => v.validate(),
            Self::DiscoveryBatch(v) => v.validate(),
            Self::DiscoveryComplete(v) => v.validate(),
            Self::DiscoveryError(v) => v.validate(),
            Self::DocumentLoad(v) => v.validate(),
            Self::RenderReady(v) => v.validate(),
            Self::RenderError(v) => v.validate(),
            Self::PositionCaptured(v) => v.validate(),
            Self::PositionRestored(v) => v.validate(),
            Self::NavigationRequest(v) => v.validate(),
            Self::NavigationResult(v) => v.validate(),
            Self::ResourceRequest(v) => v.validate(),
            Self::ResourceResult(v) => v.validate(),
            Self::ResourceRevoked(v) => v.validate(),
            Self::AppearanceUpdate(v) => v.validate(),
            Self::Action(v) => v.validate(),
            Self::Error(v) => v.validate(),
            Self::Progress(v) => v.validate(),
        }
    }
}

impl Validate for LaunchRequest {
    fn validate(&self) -> Result<(), ContractError> {
        path(&self.caller_cwd, "caller_cwd")?;
        optional_text(self.path.as_deref(), "path", MAX_PATH_BYTES)?;
        if let Some(value) = &self.path {
            path(value, "path")?;
        }
        Ok(())
    }
}
impl Validate for LaunchAck {
    fn validate(&self) -> Result<(), ContractError> {
        if matches!(self.outcome, LaunchOutcome::Accepted) && self.error.is_some() {
            return Err(ContractError::InvalidPayload("accepted launch has error"));
        }
        Ok(())
    }
}
impl Validate for DiscoveryRequest {
    fn validate(&self) -> Result<(), ContractError> {
        path(&self.root_path, "root_path")
    }
}
impl Validate for DiscoveryBatch {
    fn validate(&self) -> Result<(), ContractError> {
        list(self.entries.len(), "entries", MAX_BATCH_ITEMS)?;
        for entry in &self.entries {
            text(&entry.relative_path, "relative_path", MAX_PATH_BYTES)?;
            if entry.relative_path.starts_with('/') {
                return Err(ContractError::InvalidPath(entry.relative_path.clone()));
            }
        }
        Ok(())
    }
}
impl Validate for DiscoveryComplete {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}
impl Validate for DiscoveryError {
    fn validate(&self) -> Result<(), ContractError> {
        text(&self.message, "message", MAX_TEXT_BYTES)
    }
}
impl Validate for DocumentLoad {
    fn validate(&self) -> Result<(), ContractError> {
        path(&self.path, "path")?;
        if !self.source.is_empty() {
            text(&self.source, "source", MAX_SOURCE_BYTES)?;
        }
        if let Some(anchor) = &self.anchor {
            text(anchor, "anchor", MAX_TEXT_BYTES)?;
        }
        if let Some(locator) = &self.locator {
            locator.validate()?;
        }
        Ok(())
    }
}
impl Validate for Locator {
    fn validate(&self) -> Result<(), ContractError> {
        optional_text(self.heading.as_deref(), "heading", MAX_TEXT_BYTES)?;
        text(&self.block, "block", MAX_TEXT_BYTES)
    }
}
impl Validate for RenderReady {
    fn validate(&self) -> Result<(), ContractError> {
        list(self.headings.len(), "headings", MAX_HEADINGS)?;
        for heading in &self.headings {
            text(&heading.id, "heading_id", MAX_TEXT_BYTES)?;
            text(&heading.text, "heading_text", MAX_TEXT_BYTES)?;
            if !(1..=6).contains(&heading.level) {
                return Err(ContractError::InvalidPayload("heading level must be 1..=6"));
            }
        }
        Ok(())
    }
}
impl Validate for RenderError {
    fn validate(&self) -> Result<(), ContractError> {
        text(&self.message, "message", MAX_TEXT_BYTES)
    }
}
impl Validate for PositionCaptured {
    fn validate(&self) -> Result<(), ContractError> {
        self.locator.validate()
    }
}
impl Validate for PositionRestored {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}
impl Validate for NavigationRequest {
    fn validate(&self) -> Result<(), ContractError> {
        match &self.target {
            NavigationTarget::Anchor { value } => text(value, "anchor", MAX_TEXT_BYTES),
            NavigationTarget::Markdown { path } | NavigationTarget::LocalFile { path } => {
                text(path, "navigation_path", MAX_PATH_BYTES)
            }
            NavigationTarget::Http { url } | NavigationTarget::Mailto { url } => {
                text(url, "url", MAX_TEXT_BYTES)
            }
        }
    }
}
impl Validate for NavigationResult {
    fn validate(&self) -> Result<(), ContractError> {
        if matches!(self.decision, NavigationDecision::Accepted)
            && matches!(self.authority, NavigationAuthority::None)
        {
            return Err(ContractError::InvalidPayload(
                "accepted navigation requires native authority",
            ));
        }
        Ok(())
    }
}
impl Validate for ResourceRequest {
    fn validate(&self) -> Result<(), ContractError> {
        match &self.reference {
            ResourceReference::RelativePath { value }
            | ResourceReference::RemoteUrl { value }
            | ResourceReference::Opaque { value } => {
                text(value, "resource_reference", MAX_PATH_BYTES)
            }
        }
    }
}
impl Validate for ResourceResult {
    fn validate(&self) -> Result<(), ContractError> {
        match &self.result {
            ResourceResultValue::Bytes { mime, bytes } => {
                text(mime, "mime", MAX_TEXT_BYTES)?;
                if bytes.len() > MAX_RESOURCE_BYTES {
                    return Err(ContractError::TextTooLong {
                        field: "resource_bytes",
                        bytes: bytes.len(),
                        max: MAX_RESOURCE_BYTES,
                    });
                }
                Ok(())
            }
            ResourceResultValue::Denied { .. } => Ok(()),
        }
    }
}
impl Validate for ResourceRevoked {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}
impl Validate for AppearanceUpdate {
    fn validate(&self) -> Result<(), ContractError> {
        self.appearance.validate()
    }
}
impl Validate for Appearance {
    fn validate(&self) -> Result<(), ContractError> {
        if !(50..=300).contains(&self.scale_percent) {
            return Err(ContractError::InvalidScale(self.scale_percent));
        }
        color(&self.reader_background)?;
        color(&self.reader_foreground)?;
        color(&self.code_background)?;
        color(&self.accent)?;
        list(self.syntax.len(), "syntax", MAX_SYNTAX_TOKENS)?;
        for token in &self.syntax {
            color(&token.foreground)?;
            if let Some(background) = &token.background {
                color(background)?;
            }
        }
        Ok(())
    }
}
impl Validate for ActionMessageEnvelope {
    fn validate(&self) -> Result<(), ContractError> {
        self.action.validate()
    }
}
impl Validate for ActionMessage {
    fn validate(&self) -> Result<(), ContractError> {
        match self {
            Self::Search(SearchAction::Open { query, .. }) => text(query, "query", MAX_TEXT_BYTES),
            Self::Outline(OutlineAction::Navigate { heading }) => {
                text(heading, "heading", MAX_TEXT_BYTES)
            }
            Self::CapturePosition(v) => v.validate(),
            Self::RestorePosition(v) => v.validate(),
            _ => Ok(()),
        }
    }
}
impl Validate for CapturePosition {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}
impl Validate for RestorePosition {
    fn validate(&self) -> Result<(), ContractError> {
        self.locator.validate()
    }
}
impl Validate for ErrorMessage {
    fn validate(&self) -> Result<(), ContractError> {
        text(&self.message, "message", MAX_TEXT_BYTES)
    }
}
impl Validate for ProgressMessage {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(total) = self.total
            && self.completed > total
        {
            return Err(ContractError::InvalidPayload("completed exceeds total"));
        }
        Ok(())
    }
}

impl From<ContractError> for String {
    fn from(value: ContractError) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T>(value: u64) -> T
    where
        T: IdNew,
    {
        T::make(value)
    }
    trait IdNew {
        fn make(value: u64) -> Self;
    }
    macro_rules! id_new { ($($ty:ty),+) => { $(impl IdNew for $ty { fn make(value: u64) -> Self { <$ty>::new(value).unwrap() } })+ }; }
    id_new!(
        InvocationId,
        RootId,
        ScanId,
        DocumentId,
        Generation,
        RequestId,
        ResourceId
    );

    fn launch() -> Envelope {
        Envelope::new(Message::LaunchRequest(LaunchRequest {
            invocation: id(1),
            intent: LaunchIntent::Picker,
            caller_cwd: "/work".into(),
            path: None,
            ack_timeout_ms: 500,
        }))
    }

    #[test]
    fn canonical_round_trip_and_fixture_shape() {
        let envelope = launch();
        let bytes = encode(&envelope).unwrap();
        assert_eq!(decode(&bytes).unwrap(), envelope);
        assert_eq!(
            canonical_json(&envelope).unwrap(),
            String::from_utf8(bytes).unwrap()
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let json = br#"{"revision":1,"message":{"kind":"launch.request","payload":{"invocation":1,"intent":"picker","caller_cwd":"/work","path":null,"ack_timeout_ms":500,"extra":true}}}"#;
        assert!(decode(json).is_err());
    }

    #[test]
    fn checked_json_fixtures_have_expected_dispositions() {
        let valid = [
            include_str!("../../contracts/fixtures/v1/valid/launch-request.json"),
            include_str!("../../contracts/fixtures/v1/valid/document-load.json"),
            include_str!("../../contracts/fixtures/v1/valid/navigation-result.json"),
            include_str!("../../contracts/fixtures/v1/valid/position-capture.json"),
            include_str!("../../contracts/fixtures/v1/valid/discovery-batch.json"),
            include_str!("../../contracts/fixtures/v1/valid/resource-denied.json"),
            include_str!("../../contracts/fixtures/v1/valid/appearance.json"),
            include_str!("../../contracts/fixtures/v1/valid/action-search.json"),
            include_str!("../../contracts/fixtures/v1/valid/error.json"),
        ];
        for fixture in valid {
            let envelope = decode(fixture.as_bytes()).unwrap();
            assert_eq!(decode(&encode(&envelope).unwrap()).unwrap(), envelope);
        }

        let invalid = [
            include_str!("../../contracts/fixtures/v1/invalid/unknown-field.json"),
            include_str!("../../contracts/fixtures/v1/invalid/unknown-kind.json"),
            include_str!("../../contracts/fixtures/v1/invalid/zero-id.json"),
            include_str!("../../contracts/fixtures/v1/invalid/unknown-revision.json"),
            include_str!("../../contracts/fixtures/v1/invalid/unauthorized-navigation.json"),
        ];
        for fixture in invalid {
            assert!(
                decode(fixture.as_bytes()).is_err(),
                "fixture unexpectedly valid: {fixture}"
            );
        }
    }

    #[test]
    fn zero_ids_and_stale_results_are_rejected() {
        let json = br#"{"revision":1,"message":{"kind":"render.ready","payload":{"document":0,"generation":1,"headings":[]}}}"#;
        assert!(decode(json).is_err());
        let old = Generation::new(1).unwrap();
        let current = Generation::new(2).unwrap();
        assert!(matches!(
            reject_stale_generation(current, old),
            Err(ContractError::StaleGeneration { .. })
        ));
        let old_scan = ScanId::new(1).unwrap();
        let current_scan = ScanId::new(2).unwrap();
        assert!(matches!(
            reject_stale_scan(current_scan, old_scan),
            Err(ContractError::StaleScan { .. })
        ));
    }

    #[test]
    fn unknown_kind_and_revision_are_rejected() {
        let unknown_kind = br#"{"revision":1,"message":{"kind":"not-a-message","payload":{}}}"#;
        assert!(decode(unknown_kind).is_err());
        let unknown_revision = br#"{"revision":99,"message":{"kind":"launch.request","payload":{"invocation":1,"intent":"picker","caller_cwd":"/work","path":null,"ack_timeout_ms":500}}}"#;
        assert!(matches!(
            decode(unknown_revision),
            Err(ContractError::UnknownRevision(99))
        ));
    }

    #[test]
    fn bounds_are_rejected_at_each_boundary() {
        let source = "x".repeat(MAX_SOURCE_BYTES + 1);
        let load = Message::DocumentLoad(DocumentLoad {
            document: id(1),
            generation: id(1),
            path: "/work/readme.md".into(),
            source,
            anchor: None,
            locator: None,
        });
        assert!(matches!(
            encode(&Envelope::new(load)),
            Err(ContractError::TextTooLong {
                field: "source",
                ..
            })
        ));

        let bytes = vec![0; MAX_RESOURCE_BYTES + 1];
        let resource = Message::ResourceResult(ResourceResult {
            request: id(1),
            resource: id(1),
            document: id(1),
            generation: id(1),
            result: ResourceResultValue::Bytes {
                mime: "image/png".into(),
                bytes,
            },
        });
        assert!(matches!(
            encode(&Envelope::new(resource)),
            Err(ContractError::TextTooLong {
                field: "resource_bytes",
                ..
            })
        ));

        let entries = (0..=MAX_BATCH_ITEMS)
            .map(|_| DiscoveryEntry {
                relative_path: "doc.md".into(),
            })
            .collect();
        let batch = Message::DiscoveryBatch(DiscoveryBatch {
            root: id(1),
            scan: id(1),
            entries,
        });
        assert!(matches!(
            encode(&Envelope::new(batch)),
            Err(ContractError::TooMany {
                field: "entries",
                ..
            })
        ));
        assert!(matches!(
            decode(&vec![b' '; MAX_FRAME_BYTES + 1]),
            Err(ContractError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn position_requires_matching_ready_generation_and_native_authority() {
        let document = id(7);
        let generation = id(3);
        let ready = RenderReady {
            document,
            generation,
            headings: vec![],
        };
        assert!(ready.validate_for(document, generation).is_ok());
        assert!(matches!(
            ready.validate_for(document, id(4)),
            Err(ContractError::StaleGeneration { .. })
        ));

        let locator = Locator {
            heading: Some("intro".into()),
            block: "p-1".into(),
            offset: 4,
            fallback: LocatorFallback::NearestHeading,
        };
        let captured = PositionCaptured {
            request: id(1),
            document,
            generation,
            locator: locator.clone(),
        };
        let restored = PositionRestored {
            request: id(1),
            document,
            generation,
            restored: true,
        };
        assert!(encode(&Envelope::new(Message::PositionCaptured(captured))).is_ok());
        assert!(encode(&Envelope::new(Message::PositionRestored(restored))).is_ok());

        let result = NavigationResult {
            request: id(1),
            document,
            generation,
            decision: NavigationDecision::Accepted,
            authority: NavigationAuthority::None,
            error: None,
        };
        assert!(matches!(
            encode(&Envelope::new(Message::NavigationResult(result))),
            Err(ContractError::InvalidPayload(_))
        ));
    }

    #[test]
    fn stale_scans_reject_batches_and_completion() {
        let batch = DiscoveryBatch {
            root: id(1),
            scan: id(2),
            entries: vec![],
        };
        assert!(matches!(
            batch.validate_for(id(1), id(3)),
            Err(ContractError::StaleScan { .. })
        ));
        let complete = DiscoveryComplete {
            root: id(1),
            scan: id(2),
            matched: 0,
        };
        assert!(complete.validate_for(id(1), id(2)).is_ok());
    }

    #[test]
    fn bounds_and_closed_resource_requests_are_checked() {
        let mut request = match launch().message {
            Message::LaunchRequest(request) => request,
            _ => unreachable!(),
        };
        request.caller_cwd = "/".to_owned() + &"x".repeat(MAX_PATH_BYTES);
        assert!(encode(&Envelope::new(Message::LaunchRequest(request))).is_err());
        let request = ResourceRequest {
            request: id(1),
            resource: id(2),
            document: id(3),
            generation: id(4),
            kind: ResourceKind::Image,
            reference: ResourceReference::RelativePath {
                value: "image.png".into(),
            },
        };
        assert!(encode(&Envelope::new(Message::ResourceRequest(request))).is_ok());
    }

    #[test]
    fn fixture_messages_cover_closed_families() {
        let messages = [
            Message::LaunchRequest(LaunchRequest {
                invocation: id(1),
                intent: LaunchIntent::Picker,
                caller_cwd: "/work".into(),
                path: None,
                ack_timeout_ms: 500,
            }),
            Message::DiscoveryComplete(DiscoveryComplete {
                root: id(1),
                scan: id(2),
                matched: 0,
            }),
            Message::DocumentLoad(DocumentLoad {
                document: id(1),
                generation: id(1),
                path: "/work/readme.md".into(),
                source: String::new(),
                anchor: None,
                locator: None,
            }),
            Message::RenderReady(RenderReady {
                document: id(1),
                generation: id(1),
                headings: vec![],
            }),
            Message::NavigationRequest(NavigationRequest {
                request: id(1),
                document: id(1),
                generation: id(1),
                target: NavigationTarget::Anchor {
                    value: "intro".into(),
                },
            }),
            Message::ResourceRequest(ResourceRequest {
                request: id(1),
                resource: id(1),
                document: id(1),
                generation: id(1),
                kind: ResourceKind::Image,
                reference: ResourceReference::RelativePath {
                    value: "image.png".into(),
                },
            }),
            Message::AppearanceUpdate(AppearanceUpdate {
                document: None,
                appearance: Appearance {
                    mode: AppearanceMode::System,
                    scale_percent: 100,
                    reader_background: "#ffffff".into(),
                    reader_foreground: "#000000".into(),
                    code_background: "#eeeeee".into(),
                    accent: "#3366ff".into(),
                    syntax: vec![],
                },
            }),
            Message::Error(ErrorMessage {
                scope: ErrorScope::Render,
                code: ErrorCode::Parse,
                message: "bad markdown".into(),
                retry: Some(RetryAction::Retry),
                document: Some(id(1)),
                generation: Some(id(1)),
                scan: None,
            }),
        ];
        for message in messages {
            let envelope = Envelope::new(message);
            assert_eq!(decode(&encode(&envelope).unwrap()).unwrap(), envelope);
        }
    }
}
