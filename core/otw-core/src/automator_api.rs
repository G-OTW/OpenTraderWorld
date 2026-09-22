//! HTTP API for the Automator module.
//!
//! Session-protected like every other module API. Three groups: the workflows and their
//! revisions, the schedules (plus the agenda the calendar page reads), and the run history.
//!
//! Two calls exist only so nothing is discovered at 3 a.m.: `/test` runs a whole workflow
//! in test mode (reads execute, writes and sends report what they would have done), and
//! `/nodes/test` runs a single block the user is editing.
//!
//! ## Agents author, humans arm
//!
//! Part of this API is in the MCP catalog, because composing a graph is the hardest thing
//! the module asks of anyone and it is exactly what an agent is good at. What an agent may
//! *not* do is decide what runs with privileges, and the split is enforced here rather
//! than by a permission level, because the danger is not who calls but which token the
//! workflow runs under: a graph and its `mcp_tokens` envelope are two separate grants.
//!
//! So, for a caller carrying [`Automated`]:
//! - a graph save lands in `draft`, never in `graph`. The engine never reads a draft and
//!   the editor loads it over the graph, so a proposal is visible, reviewable and one
//!   click from adoption, while the revision running on a schedule is untouched.
//! - `mcp_token_id` is refused on PATCH: an envelope is attached by hand, after reading
//!   the graph it will run.
//! - `/run` and `/rollback` are refused outright, and so is `/nodes/test`, which would run
//!   an arbitrary block under an arbitrary workflow's envelope.
//! - `/test` is allowed on an unarmed workflow only, with notifications and external calls
//!   forced off. It therefore has no outbound channel and no privileged read: it checks
//!   that the graph runs, which is the point. A test always reads the draft when the draft
//!   is runnable, for every caller: it answers for what the editor is showing.
//!
//! The catalog exposes only the read, create, save and test half; delete, the schedules and
//! the run controls are absent from it.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use otw_store::automator::{self as store, Schedule, ScheduleInput};

use crate::automator::{self, engine, expr, nodes, schedule, Graph, RunMode};
use crate::internal_call::Automated;
use crate::mcp::catalog;
use crate::{ApiError, AppState};

/// Occurrences one agenda query may materialize.
const AGENDA_CAP: usize = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/automator/workflows", get(list_workflows).post(create_workflow))
        .route(
            "/api/automator/workflows/{id}",
            get(get_workflow)
                .put(save_graph)
                .patch(patch_workflow)
                .delete(delete_workflow),
        )
        .route("/api/automator/workflows/{id}/versions", get(list_versions))
        .route("/api/automator/workflows/{id}/rollback", post(rollback))
        .route("/api/automator/workflows/{id}/run", post(run_now))
        .route("/api/automator/workflows/{id}/test", post(test_run))
        .route("/api/automator/nodes/test", post(test_node))
        .route("/api/automator/catalog", get(catalog_list))
        .route("/api/automator/catalog/schema", get(catalog_schema))
        .route("/api/automator/timezones", get(timezones))
        .route("/api/automator/schedules", get(list_schedules).post(create_schedule))
        .route(
            "/api/automator/schedules/{id}",
            axum::routing::patch(update_schedule).delete(delete_schedule),
        )
        .route("/api/automator/agenda", get(agenda))
        .route("/api/automator/runs", get(list_runs))
        .route("/api/automator/runs/{id}", get(get_run))
        .route("/api/automator/runs/{id}/cancel", post(cancel_run))
        .route("/api/automator/runs/{id}/blob/{blob_id}", get(get_blob))
}

// ── Workflows ────────────────────────────────────────────────────────────────

async fn list_workflows(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let workflows = store::list_workflows(&state.pool).await?;
    let schedules = store::list_schedules(&state.pool, None).await?;
    let last = store::last_runs(&state.pool).await?;
    Ok(Json(json!({ "workflows": workflows, "schedules": schedules, "last_runs": last })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateInput {
    name: String,
    #[serde(default)]
    description: String,
}

async fn create_workflow(
    State(state): State<AppState>,
    Json(input): Json<CreateInput>,
) -> Result<Json<Value>, ApiError> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("the workflow needs a name"));
    }
    let workflow =
        store::create_workflow(&state.pool, name, input.description.trim(), &json!({ "nodes": [], "edges": [] }))
            .await?;
    Ok(Json(json!({ "workflow": workflow })))
}

async fn get_workflow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    automated: Option<Extension<Automated>>,
) -> Result<Json<Value>, ApiError> {
    let workflow = load(&state, id).await?;
    let schedules = store::list_schedules(&state.pool, Some(id)).await?;
    let runs = store::list_runs(&state.pool, Some(id), None, 20).await?;
    // The token list is here for the editor's "attach an envelope" picker. An agent cannot
    // attach one, and has no business reading the inventory of what exists, so it is left
    // out rather than sent and ignored.
    let tokens = match automated {
        Some(_) => Value::Null,
        None => serde_json::to_value(otw_store::mcp::list_tokens(&state.pool).await?)
            .unwrap_or(Value::Null),
    };
    // A draft that parses can only have come from an agent: autosave parks a graph in the
    // draft precisely because it did *not* parse, and saves it for real when it did. So the
    // editor can tell "a block is half-written" from "someone proposed this, read it" with
    // no column recording who wrote what.
    let draft_valid = workflow.draft.as_ref().is_some_and(|d| parse_graph(d).is_ok());
    Ok(Json(json!({
        "workflow": workflow,
        "draft_valid": draft_valid,
        "schedules": schedules,
        "runs": runs,
        "tokens": tokens,
    })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct SaveInput {
    graph: Value,
    #[serde(default)]
    note: String,
    /// Point the workflow's schedules at this new revision. The UI asks, because a pinned
    /// schedule keeps running the old graph until someone says otherwise.
    #[serde(default)]
    repoint_schedules: bool,
    /// The editor's autosave. A graph that does not validate is parked as a draft instead
    /// of being refused, because half a block is what autosave keeps catching; a save the
    /// user asked for still fails loudly.
    #[serde(default)]
    draft: bool,
}

async fn save_graph(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    automated: Option<Extension<Automated>>,
    Json(input): Json<SaveInput>,
) -> Result<Json<Value>, ApiError> {
    // An agent proposes, it does not arm: the graph lands in the draft the engine never
    // reads and the editor loads first. Nothing that runs today changes, and adopting the
    // proposal is the human's ordinary Save. The graph is still parsed, so the agent gets
    // the validation error rather than the silent park autosave gets: a proposal it cannot
    // be told is broken is worse than no proposal.
    if automated.is_some() {
        let graph = parse_graph(&input.graph)?;
        let normalized = serde_json::to_value(&graph).unwrap_or_else(|_| input.graph.clone());
        let Some(workflow) = store::save_draft(&state.pool, id, &normalized).await? else {
            return Err(ApiError::not_found("workflow not found"));
        };
        return Ok(Json(json!({
            "workflow": workflow,
            "draft": true,
            "reason": "saved as a draft: the workflow's owner opens it in the editor and saves \
                       it to make this the graph that runs",
        })));
    }
    let graph = match parse_graph(&input.graph) {
        Ok(g) => g,
        Err(e) if input.draft => {
            let Some(workflow) = store::save_draft(&state.pool, id, &input.graph).await? else {
                return Err(ApiError::not_found("workflow not found"));
            };
            return Ok(Json(json!({ "workflow": workflow, "draft": true, "reason": e.message })));
        }
        Err(e) => return Err(e),
    };
    let normalized = serde_json::to_value(&graph).unwrap_or(input.graph.clone());
    let Some((workflow, version)) =
        store::save_graph(&state.pool, id, &normalized, input.note.trim()).await?
    else {
        return Err(ApiError::not_found("workflow not found"));
    };
    if input.repoint_schedules {
        store::repoint_schedules(&state.pool, id, Some(version.id)).await?;
    }
    Ok(Json(json!({ "workflow": workflow, "version": version })))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PatchInput {
    name: Option<String>,
    description: Option<String>,
    favorite: Option<bool>,
    enabled: Option<bool>,
    max_runtime_secs: Option<i32>,
    /// `null` clears the permission envelope, absent leaves it alone. Refused outright
    /// from an automated caller, whatever the value: the envelope is what turns a graph
    /// into privileges, so it is granted by hand or not at all.
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    #[schemars(with = "Option<Uuid>")]
    mcp_token_id: Option<Option<Uuid>>,
}

async fn patch_workflow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    automated: Option<Extension<Automated>>,
    Json(input): Json<PatchInput>,
) -> Result<Json<Value>, ApiError> {
    if automated.is_some() && input.mcp_token_id.is_some() {
        return Err(ApiError::forbidden(
            "a workflow's access token is attached by hand, in its settings",
        ));
    }
    if let Some(name) = &input.name {
        if name.trim().is_empty() {
            return Err(ApiError::bad_request("the workflow needs a name"));
        }
    }
    if let Some(secs) = input.max_runtime_secs {
        if !(10..=3600).contains(&secs) {
            return Err(ApiError::bad_request("the run limit must be between 10 s and 1 h"));
        }
    }
    let workflow = store::update_workflow(
        &state.pool,
        id,
        input.name.as_deref().map(str::trim),
        input.description.as_deref().map(str::trim),
        input.favorite,
        input.enabled,
        input.max_runtime_secs,
        input.mcp_token_id,
    )
    .await?
    .ok_or_else(|| ApiError::not_found("workflow not found"))?;
    Ok(Json(json!({ "workflow": workflow })))
}

async fn delete_workflow(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_workflow(&state.pool, id).await? {
        return Err(ApiError::not_found("workflow not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn list_versions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "versions": store::list_versions(&state.pool, id).await? })))
}

#[derive(Deserialize)]
struct RollbackInput {
    version_id: Uuid,
}

/// Rolling back appends the old graph as a new revision rather than deleting history, the
/// same way the prompt store does it.
async fn rollback(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    automated: Option<Extension<Automated>>,
    Json(input): Json<RollbackInput>,
) -> Result<Json<Value>, ApiError> {
    // Writes the live graph, so it is the same decision as a save: a human's.
    if automated.is_some() {
        return Err(ApiError::forbidden("a revision is restored from the workflow's page"));
    }
    let graph = store::version_graph(&state.pool, input.version_id)
        .await?
        .ok_or_else(|| ApiError::not_found("revision not found"))?;
    let Some((workflow, version)) =
        store::save_graph(&state.pool, id, &graph, "rollback").await?
    else {
        return Err(ApiError::not_found("workflow not found"));
    };
    Ok(Json(json!({ "workflow": workflow, "version": version })))
}

// ── Running ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct RunInput {
    /// Values readable from a block as `{{input.<key>}}`.
    #[serde(default)]
    input: Value,
}

async fn run_now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    automated: Option<Extension<Automated>>,
    body: Option<Json<RunInput>>,
) -> Result<Json<Value>, ApiError> {
    // Absent from the catalog, so unreachable anyway; stated here because this is the one
    // call that spends the workflow's whole envelope for real.
    if automated.is_some() {
        return Err(ApiError::forbidden("a workflow is run by hand, from its page"));
    }
    let workflow = load(&state, id).await?;
    let graph = parse_graph(&workflow.graph)?;
    if graph.nodes.is_empty() {
        return Err(ApiError::bad_request("this workflow has no blocks yet"));
    }
    let input = body.map(|b| b.0.input).unwrap_or(Value::Null);
    let input = if input.is_object() { input } else { json!({}) };

    let run = store::start_run(
        &state.pool,
        id,
        workflow.version_id,
        None,
        "manual",
        "live",
        &input,
    )
    .await?
    .ok_or_else(|| ApiError::conflict("this workflow is already running"))?;

    engine::spawn(state.clone(), workflow, graph, run.clone(), RunMode::Live);
    Ok(Json(json!({ "run": run })))
}

#[derive(Deserialize, Default, schemars::JsonSchema)]
pub(crate) struct TestInput {
    #[serde(default)]
    input: Value,
    /// Send real notifications during the test (off by default).
    #[serde(default)]
    send_notifications: bool,
    /// Execute external GET calls during the test (on by default: reading is the point).
    #[serde(default = "yes")]
    call_external: bool,
}

fn yes() -> bool {
    true
}

/// Run the whole workflow in test mode and answer with the complete trace. Synchronous:
/// the user is watching, and the run's own deadline bounds it.
async fn test_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    automated: Option<Extension<Automated>>,
    body: Option<Json<TestInput>>,
) -> Result<Json<Value>, ApiError> {
    let workflow = load(&state, id).await?;
    let mut opts = body.map(|b| b.0).unwrap_or_default();

    if automated.is_some() {
        // Test mode executes reads for real, under the workflow's own envelope. On an armed
        // workflow that would let a caller read through a token it was never granted, so
        // that test belongs to whoever holds the envelope.
        if workflow.mcp_token_id.is_some() {
            return Err(ApiError::forbidden(
                "this workflow carries an access token: it is tested from its own page",
            ));
        }
        // Sealed: `notify` reports what it would send, `http` reports what it would call,
        // and with no envelope an `api` block fails closed. What is left, the topology, the
        // expressions and the transforms, is exactly what a proposal needs checked.
        opts.send_notifications = false;
        opts.call_external = false;
    }

    // The draft is what the editor is showing and what an agent just wrote, so it is what a
    // test has to answer for. One that does not parse is not a graph to run at all, and the
    // stored one stands: that is the half-written block autosave parked.
    let graph = match workflow.draft.as_ref().and_then(|d| parse_graph(d).ok()) {
        Some(g) => g,
        None => parse_graph(&workflow.graph)?,
    };
    if graph.nodes.is_empty() {
        return Err(ApiError::bad_request("this workflow has no blocks yet"));
    }
    let input = if opts.input.is_object() { opts.input.clone() } else { json!({}) };
    let mode = RunMode::Test {
        send_notifications: opts.send_notifications,
        call_external: opts.call_external,
    };
    let run = store::start_run(
        &state.pool,
        id,
        workflow.version_id,
        None,
        "test",
        "test",
        &input,
    )
    .await?
    .ok_or_else(|| ApiError::conflict("this workflow is already running"))?;

    let result = engine::execute(&state, &workflow, &graph, &run, mode).await;
    Ok(Json(json!({
        "run_id": run.id,
        "status": result.status,
        "error": result.error,
        "steps": result.steps.iter().map(step_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct NodeTestInput {
    node: Value,
    /// The workflow the block belongs to, for its permission envelope.
    #[serde(default)]
    workflow_id: Option<Uuid>,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    send_notifications: bool,
    #[serde(default = "yes")]
    call_external: bool,
}

/// Run one block on its own. Upstream references are unavailable here by construction, so
/// a block that reads `{{steps.…}}` reports that plainly instead of pretending.
async fn test_node(
    State(state): State<AppState>,
    automated: Option<Extension<Automated>>,
    Json(body): Json<NodeTestInput>,
) -> Result<Json<Value>, ApiError> {
    // Runs an arbitrary block under an arbitrary workflow's envelope, which is the whole
    // escalation in one call. Kept out of the catalog and refused here as well.
    if automated.is_some() {
        return Err(ApiError::forbidden("a single block is tested from the editor"));
    }
    let node: automator::Node = serde_json::from_value(body.node)
        .map_err(|e| ApiError::bad_request(&format!("invalid block: {e}")))?;
    nodes::validate_config(&node.kind, &node.config).map_err(|e| ApiError::bad_request(&e))?;

    let (perms, token_name) = match body.workflow_id {
        Some(id) => {
            let workflow = load(&state, id).await?;
            match workflow.mcp_token_id {
                Some(token_id) => match otw_store::mcp::get_token(&state.pool, token_id).await? {
                    Some(token) if !token.is_expired() => (token.permissions, token.name),
                    Some(token) => (json!({}), token.name),
                    None => (json!({}), String::new()),
                },
                None => (json!({}), String::new()),
            }
        }
        None => (json!({}), String::new()),
    };

    let secrets = expr::Secrets::default();
    let resolver = expr::Resolver::new(&state, secrets.clone());
    let vars = expr::Ctx {
        steps: serde_json::Map::new(),
        run: json!({ "id": Uuid::nil().to_string(), "trigger": "test", "mode": "test",
                     "started_at": now_rfc3339() }),
        input: if body.input.is_object() { body.input.clone() } else { json!({}) },
        workflow: json!({ "name": "test" }),
        ..Default::default()
    };
    let ctx = nodes::NodeCtx {
        state: &state,
        // A standalone block test belongs to no run; the nil id simply never matches one.
        run_id: Uuid::nil(),
        mode: RunMode::Test {
            send_notifications: body.send_notifications,
            call_external: body.call_external,
        },
        node: &node,
        vars: &vars,
        resolver: &resolver,
        perms,
        token_name,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
    };

    match nodes::run(&ctx).await {
        Ok(mut out) => {
            secrets.scrub_value(&mut out.request);
            secrets.scrub_value(&mut out.output);
            Ok(Json(json!({
                "status": if out.simulated { "simulated" } else { "ok" },
                "request": out.request,
                "output": out.output,
            })))
        }
        Err(e) => Ok(Json(json!({ "status": "failed", "error": secrets.scrub(&e) }))),
    }
}

// ── Catalog ──────────────────────────────────────────────────────────────────

/// The block palette: the native block types plus every endpoint a workflow may call.
/// Generated from the MCP catalog, so a new allowlisted endpoint appears here on its own
/// and an excluded one can never be reached from a workflow.
async fn catalog_list(State(_state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let endpoints: Vec<Value> = catalog::CATALOG
        .iter()
        .map(|e| {
            json!({
                "module": e.module,
                "method": e.method,
                "path": e.path,
                "desc": e.desc,
                "compute": e.compute,
                "has_body": e.body.is_some(),
            })
        })
        .collect();
    let modules: Vec<Value> = catalog::MODULES
        .iter()
        .map(|(id, label)| json!({ "id": id, "label": label }))
        .collect();
    Ok(Json(json!({
        "kinds": automator::KINDS,
        "ops": { "transform": nodes::transform::OPS, "condition": nodes::branch::OPS },
        "csv_shapes": nodes::transform::CSV_SHAPES,
        "http_methods": nodes::http::METHODS,
        "parsers": nodes::http::PARSERS,
        "modules": modules,
        "endpoints": endpoints,
    })))
}

#[derive(Deserialize)]
struct SchemaQuery {
    method: String,
    path: String,
}

/// One endpoint's request-body schema, fetched when a block is opened rather than shipped
/// with the whole palette (the schemas are far larger than the listing).
async fn catalog_schema(Query(q): Query<SchemaQuery>) -> Result<Json<Value>, ApiError> {
    let method = q.method.to_ascii_uppercase();
    let endpoint = catalog::lookup(&method, q.path.trim())
        .ok_or_else(|| ApiError::not_found("this endpoint is not reachable from a workflow"))?;
    Ok(Json(json!({
        "module": endpoint.module,
        "method": endpoint.method,
        "path": endpoint.path,
        "desc": endpoint.desc,
        "compute": endpoint.compute,
        "schema": endpoint.body.map(|f| f()).unwrap_or(Value::Null),
    })))
}

async fn timezones() -> Json<Value> {
    Json(json!({ "timezones": schedule::timezone_names() }))
}

// ── Schedules ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ScheduleQuery {
    #[serde(default)]
    workflow: Option<Uuid>,
}

async fn list_schedules(
    State(state): State<AppState>,
    Query(q): Query<ScheduleQuery>,
) -> Result<Json<Value>, ApiError> {
    let schedules = store::list_schedules(&state.pool, q.workflow).await?;
    let workflows = store::list_workflows(&state.pool).await?;
    Ok(Json(json!({ "schedules": schedules, "workflows": workflows })))
}

/// The rule fields are doubly optional on purpose: absent leaves the stored value alone,
/// an explicit `null` clears it. Without that, switching a rule from interval to daily
/// could never drop the interval it used to carry.
#[derive(Deserialize)]
struct ScheduleBody {
    workflow_id: Option<Uuid>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    version_id: Option<Option<Uuid>>,
    kind: Option<String>,
    timezone: Option<String>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    every_minutes: Option<Option<i32>>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    at_hour: Option<Option<i32>>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    at_minute: Option<Option<i32>>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    weekdays: Option<Option<i16>>,
    #[serde(default, deserialize_with = "otw_store::agent::double_option")]
    day_of_month: Option<Option<i32>>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    run_at: Option<OffsetDateTime>,
    catch_up: Option<bool>,
    active: Option<bool>,
}

/// Turn a request body into a rule, resolve its next occurrence, and refuse anything the
/// scheduler could not act on later.
fn to_input(body: &ScheduleBody, base: Option<&Schedule>) -> Result<ScheduleInput, ApiError> {
    let workflow_id = body
        .workflow_id
        .or(base.map(|b| b.workflow_id))
        .ok_or_else(|| ApiError::bad_request("workflow_id is required"))?;
    let input = ScheduleInput {
        workflow_id,
        version_id: body.version_id.unwrap_or_else(|| base.and_then(|b| b.version_id)),
        kind: body
            .kind
            .clone()
            .or_else(|| base.map(|b| b.kind.clone()))
            .unwrap_or_else(|| "daily".into()),
        timezone: body
            .timezone
            .clone()
            .or_else(|| base.map(|b| b.timezone.clone()))
            .unwrap_or_else(|| "UTC".into()),
        every_minutes: body.every_minutes.unwrap_or_else(|| base.and_then(|b| b.every_minutes)),
        at_hour: body.at_hour.unwrap_or_else(|| base.and_then(|b| b.at_hour)),
        at_minute: body.at_minute.unwrap_or_else(|| base.and_then(|b| b.at_minute)),
        weekdays: body.weekdays.unwrap_or_else(|| base.and_then(|b| b.weekdays)),
        day_of_month: body.day_of_month.unwrap_or_else(|| base.and_then(|b| b.day_of_month)),
        run_at: body.run_at.or(base.and_then(|b| b.run_at)),
        catch_up: body.catch_up.or(base.map(|b| b.catch_up)).unwrap_or(false),
        active: body.active.or(base.map(|b| b.active)).unwrap_or(true),
        next_run_at: None,
    };
    let probe = probe_schedule(&input, base);
    schedule::validate(&probe).map_err(|e| ApiError::bad_request(&e))?;
    let next = schedule::next_from_now(&probe);
    // A rule with no next occurrence is refused when it is written for the first time, or
    // when the instant itself is being set: a `once` in the past would otherwise be stored
    // active and silently never fire. A rule that has already fired keeps its row, so
    // deactivating or renaming it later still works.
    if next.is_none() && (base.is_none() || body.run_at.is_some()) {
        return Err(ApiError::bad_request(if input.kind == "once" {
            "that moment has already passed"
        } else {
            "this rule never comes round again"
        }));
    }
    Ok(ScheduleInput { next_run_at: next, ..input })
}

/// A row-shaped copy of an input, so rule maths has one implementation.
fn probe_schedule(input: &ScheduleInput, base: Option<&Schedule>) -> Schedule {
    Schedule {
        id: base.map(|b| b.id).unwrap_or_else(Uuid::nil),
        workflow_id: input.workflow_id,
        version_id: input.version_id,
        kind: input.kind.clone(),
        timezone: input.timezone.clone(),
        every_minutes: input.every_minutes,
        at_hour: input.at_hour,
        at_minute: input.at_minute,
        weekdays: input.weekdays,
        day_of_month: input.day_of_month,
        run_at: input.run_at,
        catch_up: input.catch_up,
        active: input.active,
        next_run_at: None,
        last_run_at: base.and_then(|b| b.last_run_at),
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
    }
}

async fn create_schedule(
    State(state): State<AppState>,
    Json(body): Json<ScheduleBody>,
) -> Result<Json<Value>, ApiError> {
    let input = to_input(&body, None)?;
    load(&state, input.workflow_id).await?;
    let row = store::create_schedule(&state.pool, &input).await?;
    Ok(Json(json!({ "schedule": row })))
}

async fn update_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ScheduleBody>,
) -> Result<Json<Value>, ApiError> {
    let existing = store::get_schedule(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("schedule not found"))?;
    let input = to_input(&body, Some(&existing))?;
    let row = store::update_schedule(&state.pool, id, &input)
        .await?
        .ok_or_else(|| ApiError::not_found("schedule not found"))?;
    Ok(Json(json!({ "schedule": row })))
}

async fn delete_schedule(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_schedule(&state.pool, id).await? {
        return Err(ApiError::not_found("schedule not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AgendaQuery {
    #[serde(with = "time::serde::rfc3339")]
    from: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    to: OffsetDateTime,
    #[serde(default)]
    workflow: Option<Uuid>,
}

/// Upcoming occurrences in a window. Materialized server-side so the timezone maths has
/// exactly one implementation, and capped so a one-minute rule over a year cannot ask for
/// half a million rows.
async fn agenda(
    State(state): State<AppState>,
    Query(q): Query<AgendaQuery>,
) -> Result<Json<Value>, ApiError> {
    if q.to <= q.from {
        return Err(ApiError::bad_request("the window ends before it starts"));
    }
    if q.to - q.from > time::Duration::days(400) {
        return Err(ApiError::bad_request("the window cannot span more than 400 days"));
    }
    let schedules = store::list_schedules(&state.pool, q.workflow).await?;
    let workflows = store::list_workflows(&state.pool).await?;
    let mut events = Vec::new();
    let mut truncated = false;
    for s in schedules.iter().filter(|s| s.active) {
        let hits = schedule::occurrences(s, q.from, q.to, AGENDA_CAP);
        if hits.len() >= AGENDA_CAP {
            truncated = true;
        }
        let name = workflows
            .iter()
            .find(|w| w.id == s.workflow_id)
            .map(|w| w.name.clone())
            .unwrap_or_default();
        for at in hits {
            events.push(json!({
                "schedule_id": s.id,
                "workflow_id": s.workflow_id,
                "workflow": name,
                "at": rfc3339(at),
                "timezone": s.timezone,
            }));
        }
    }
    events.sort_by(|a, b| a["at"].as_str().unwrap_or("").cmp(b["at"].as_str().unwrap_or("")));
    Ok(Json(json!({ "events": events, "truncated": truncated })))
}

// ── Runs ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RunsQuery {
    #[serde(default)]
    workflow: Option<Uuid>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

async fn list_runs(
    State(state): State<AppState>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<Value>, ApiError> {
    let runs =
        store::list_runs(&state.pool, q.workflow, q.status.as_deref(), q.limit.clamp(1, 200))
            .await?;
    let workflows = store::list_workflows(&state.pool).await?;
    Ok(Json(json!({ "runs": runs, "workflows": workflows })))
}

async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let run = store::get_run(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("run not found"))?;
    let steps = store::run_steps(&state.pool, id).await?;
    let workflow = store::get_workflow(&state.pool, run.workflow_id).await?;
    Ok(Json(json!({ "run": run, "steps": steps, "workflow": workflow })))
}

async fn cancel_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::request_cancel(&state.pool, id).await? {
        return Err(ApiError::bad_request("this run is already finished"));
    }
    Ok(Json(json!({ "ok": true })))
}

/// Download a payload a block parked out of its step output.
async fn get_blob(
    State(state): State<AppState>,
    Path((run_id, blob_id)): Path<(Uuid, Uuid)>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    let (content_type, data) = store::get_blob(&state.pool, run_id, blob_id)
        .await?
        .ok_or_else(|| ApiError::not_found("payload not found"))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, content_type)],
        data,
    )
        .into_response())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn load(state: &AppState, id: Uuid) -> Result<store::Workflow, ApiError> {
    store::get_workflow(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("workflow not found"))
}

/// Parse and validate a graph, turning every engine complaint into a 400 the editor shows.
fn parse_graph(value: &Value) -> Result<Graph, ApiError> {
    let graph = Graph::parse(value).map_err(|e| ApiError::bad_request(&e))?;
    automator::validate(&graph).map_err(|e| ApiError::bad_request(&e))?;
    Ok(graph)
}

/// A step as the trace view reads it (the stored row shape, minus the ids it does not need).
fn step_json(step: &otw_store::automator::StepRecord) -> Value {
    json!({
        "node_id": step.node_id,
        "node_name": step.node_name,
        "kind": step.kind,
        "seq": step.seq,
        "status": step.status,
        "request": step.request,
        "output": step.output,
        "error": step.error,
        "bytes": step.bytes,
        "duration_ms": step.duration_ms,
    })
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

fn now_rfc3339() -> String {
    rfc3339(OffsetDateTime::now_utc())
}
