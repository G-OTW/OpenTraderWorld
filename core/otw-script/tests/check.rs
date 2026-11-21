use std::collections::BTreeSet;

use otw_script::registry::{all, lookup, lookup_call, Kind, Lookup, CURRENT_PHASE};

// ── Registry (Task 5) ──

#[test]
fn lookup_supported() {
    match lookup("ta.sma") {
        Lookup::Supported(b) => assert_eq!(b.phase, 1),
        other => panic!("{other:?}"),
    }
    for name in ["close", "bar_index", "math.max", "color.new", "plot", "input.int", "timeframe.change", "syminfo.mintick", "plot.style_histogram", "color.red"] {
        assert!(matches!(lookup(name), Lookup::Supported(_)), "{name}");
    }
}

#[test]
fn lookup_not_yet() {
    assert_eq!(lookup("ta.kcw"), Lookup::NotYet { phase: 4, visual_only: false });
    assert!(matches!(lookup("label.new"), Lookup::Supported(_)));
    assert_eq!(lookup("request.security"), Lookup::NotYet { phase: 6, visual_only: false });
    assert_eq!(lookup("matrix.new"), Lookup::NotYet { phase: 0, visual_only: false });
    assert_eq!(lookup("alertcondition"), Lookup::NotYet { phase: 0, visual_only: true });
    assert!(matches!(lookup("log.info"), Lookup::Supported(_)));
    assert_eq!(lookup("math.random"), Lookup::NotYet { phase: 0, visual_only: false });
}

#[test]
fn lookup_unknown() {
    assert_eq!(lookup("ta_sma"), Lookup::Unknown);
    assert_eq!(lookup("plott"), Lookup::Unknown);
}

#[test]
fn noncausal_flags() {
    let causal = |n: &str| match lookup(n) {
        Lookup::Supported(b) => b.causal,
        other => panic!("{n}: {other:?}"),
    };
    assert!(!causal("barstate.islast"));
    assert!(!causal("barstate.islastconfirmedhistory"));
    assert!(!causal("last_bar_index"));
    assert!(!causal("last_bar_time"));
    assert!(!causal("timenow"));
    assert!(causal("close"));
    assert!(causal("ta.pivothigh"));
}

#[test]
fn no_duplicate_names() {
    let mut seen = BTreeSet::new();
    // A name may exist once as a variable and once as a function (`time`, `hour`…).
    for b in all() {
        assert!(seen.insert((b.name, b.kind == Kind::Func)), "duplicate {}", b.name);
        assert!(b.phase <= CURRENT_PHASE || b.phase == 0 || b.visual_only, "{} listed but not supported", b.name);
    }
}

#[test]
fn variable_and_function_forms() {
    assert!(matches!(lookup("hour"), Lookup::Supported(b) if b.kind == Kind::Var));
    assert!(matches!(lookup_call("hour"), Lookup::Supported(b) if b.kind == Kind::Func));
    assert!(matches!(lookup_call("ta.sma"), Lookup::Supported(_)));
    assert_eq!(lookup_call("ta.kcw"), Lookup::NotYet { phase: 4, visual_only: false });
}

// ── Checker: resolution, scopes, unsupported (Task 6) ──

use otw_script::check::check;
use otw_script::diag::{DiagCode, Diagnostic, Severity};
use otw_script::parser::parse;

/// Check a body placed after `//@version=6` + `indicator("t")` (body line 1 = source line 3).
fn diags(body: &str) -> Vec<Diagnostic> {
    check(&parse(&format!("//@version=6\nindicator(\"t\")\n{body}")).unwrap_or_else(|e| panic!("{e:?}"))).diags
}
fn errors(body: &str) -> Vec<Diagnostic> {
    diags(body).into_iter().filter(|d| d.severity == Severity::Error).collect()
}

#[test]
fn clean_script_has_no_errors() {
    assert_eq!(errors("len = input.int(14)\nx = ta.sma(close, len)\nplot(x)"), vec![]);
}

#[test]
fn all_occurrences_reported() {
    let e = errors("a = ta.kcw(close, 20, 2)\nb = ta.kcw(close, 10, 2)\nplot(a + b)");
    assert_eq!(e.len(), 2);
    assert!(e.iter().all(|d| d.code == DiagCode::Unsupported && d.args == vec!["ta.kcw".to_string(), "4".to_string()]));
    assert_eq!((e[0].span.line, e[1].span.line), (3, 4));
}

#[test]
fn unknown_name() {
    let e = errors("plot(ta_sma(close, 2))");
    assert_eq!(e[0].code, DiagCode::UnknownName);
    assert_eq!(e[0].args, vec!["ta_sma".to_string()]);
    assert_eq!(errors("plot(clsoe)")[0].code, DiagCode::UnknownName);
}

#[test]
fn unused_function_with_unsupported_is_warning() {
    let d = diags("f() => matrix.new<float>(1, 1)\nplot(close)");
    assert!(d.iter().all(|d| d.severity == Severity::Warning), "{d:?}");
    assert!(d.iter().any(|d| d.code == DiagCode::SkippedUnused && d.args == vec!["f".to_string()]));
    // Reached through another function: checked.
    assert!(!errors("f() => matrix.new<float>(1, 1)\ng() => f()\nplot(g())").is_empty());
}

#[test]
fn strategy_refused() {
    let d = check(&parse("//@version=6\nstrategy(\"s\")\nplot(close)").unwrap()).diags;
    assert!(d.iter().any(|d| d.code == DiagCode::NotIndicator));
    let d = check(&parse("//@version=6\nplot(close)").unwrap()).diags;
    assert!(d.iter().any(|d| d.code == DiagCode::NotIndicator), "no indicator() call");
}

#[test]
fn assign_undeclared() {
    assert_eq!(errors("x := 1")[0].code, DiagCode::AssignUndeclared);
    assert_eq!(errors("x += 1")[0].code, DiagCode::AssignUndeclared);
}

#[test]
fn redeclared() {
    assert_eq!(errors("x = 1\nx = 2")[0].code, DiagCode::Redeclared);
}

#[test]
fn scopes_shadow_in_function() {
    // A parameter may shadow a global; block locals do not leak out of their block.
    assert_eq!(errors("x = 1\nf(x) => x * 2\nplot(f(close))"), vec![]);
    assert_eq!(errors("if close > open\n    y = 1\nplot(y)")[0].code, DiagCode::UnknownName);
    assert_eq!(errors("for i = 0 to 3\n    z = i\nplot(i)")[0].code, DiagCode::UnknownName);
}

#[test]
fn language_features_not_yet() {
    let e = errors("varip v = 0\nplot(v)");
    assert_eq!(e[0].args, vec!["varip".to_string(), "0".to_string()]);
    // Used in a kept statement: refused. Unused (dead code): skipped, see Task 8.
    let e = errors("import a/b/1 as lib\nplot(lib.f(close))");
    assert_eq!(e[0].args[0], "import");
    let e = errors("m = matrix.new<float>(2, 2)\nplot(m.rows())");
    assert!(e.iter().any(|d| d.args.first().map(String::as_str) == Some("matrix.new")), "{e:?}");
    let e = errors("type P\n    float x\np = P.new(close)\nplot(p.x)");
    assert!(e.iter().all(|d| d.args[0] == "type"), "{e:?}");
    assert!(errors("type P\n    float x\nplot(close)").is_empty(), "unused type: skipped");
}

// ── Corpus regressions for the checker ──

#[test]
fn underscore_placeholder_repeats() {
    assert_eq!(errors("[_, _, h] = ta.macd(close, 12, 26, 9)\n[a, _, _] = ta.macd(close, 5, 10, 3)\nplot(h + a)"), vec![]);
}

#[test]
fn color_cast_and_tr_call() {
    assert_eq!(errors("c = color.new(color(#DBD07C), 0)\nplot(ta.tr(true), color=c)"), vec![]);
}

#[test]
fn user_type_constructor_is_type_feature() {
    let e = errors("type P\n    float x\np = P.new(1.0)\nplot(close)");
    assert!(e.iter().all(|d| d.code == DiagCode::Unsupported && d.args[0] == "type"), "{e:?}");
}

#[test]
fn library_alias_is_import_feature() {
    let e = errors("import TradingView/ta/7 as tv\nplot(tv.relativeVolume(20))");
    assert!(e.iter().all(|d| d.code == DiagCode::Unsupported && d.args[0] == "import"), "{e:?}");
}

#[test]
fn alert_frequency_constant_is_visual() {
    let e = errors("if close > open\n    alert(\"x\", alert.freq_once_per_bar_close)\nplot(close)");
    assert_eq!(e, vec![]);
}

#[test]
fn max_bars_back_and_drawing_casts() {
    assert_eq!(errors("max_bars_back(close, 500)\nl = label(na)\nplot(close)"), vec![]);
}

// ── Types, arity, named args (Task 7) ──

fn codes(body: &str) -> Vec<DiagCode> {
    errors(body).into_iter().map(|d| d.code).collect()
}

#[test]
fn arity() {
    assert_eq!(codes("plot(ta.sma(close))"), vec![DiagCode::ArgCount]);
    assert_eq!(codes("plot(ta.sma(close, 1, 2))"), vec![DiagCode::ArgCount]);
    assert_eq!(codes("f(a, b = 2) => a + b\nplot(f())"), vec![DiagCode::ArgCount]);
    assert_eq!(codes("plot(math.max(1, 2, 3, 4))"), vec![], "variadic");
}

#[test]
fn bad_named_arg() {
    assert_eq!(codes("plot(close, titel=\"x\")"), vec![DiagCode::ArgName]);
    assert_eq!(codes("plot(close, title=\"x\", color=color.red, linewidth=2)"), vec![]);
}

#[test]
fn default_params_fill() {
    assert_eq!(codes("plot(ta.stdev(close, 20))"), vec![]);
    assert_eq!(codes("plot(ta.highest(10))"), vec![], "ta.highest(length): source defaults to high");
    assert_eq!(codes("plot(ta.pivothigh(2, 2))"), vec![]);
    assert_eq!(codes("plot(ta.change(close))"), vec![]);
}

#[test]
fn ternary_branch_types() {
    assert_eq!(codes("x = close > open ? 1 : \"a\"\nplot(close)"), vec![DiagCode::TypeMismatch]);
    assert_eq!(codes("x = close > open ? 1 : na\nplot(x)"), vec![]);
}

#[test]
fn int_to_float_ok() {
    assert_eq!(codes("float x = 1\nx := bar_index\nplot(x)"), vec![]);
    assert_eq!(codes("int i = 1.5\nplot(i)"), vec![DiagCode::TypeMismatch]);
}

#[test]
fn v6_bool_na_refused_v5_ok() {
    assert_eq!(codes("bool b = na\nplot(close)"), vec![DiagCode::TypeMismatch]);
    let d = check(&parse("//@version=5\nindicator(\"t\")\nbool b = na\nplot(close)").unwrap()).diags;
    assert!(d.iter().all(|d| !d.is_error()), "{d:?}");
}

#[test]
fn tuple_destructure_count() {
    assert_eq!(codes("[a, b] = ta.macd(close, 12, 26, 9)\nplot(a)"), vec![DiagCode::TypeMismatch]);
    assert_eq!(codes("f() => [1, 2]\n[a, b, c] = f()\nplot(a)"), vec![DiagCode::TypeMismatch]);
    assert_eq!(codes("f() => [1, 2]\n[a, b] = f()\nplot(a)"), vec![]);
}

// ── Corpus regressions for types and arguments ──

#[test]
fn valuewhen_returns_source_type() {
    assert_eq!(codes("int b = ta.valuewhen(close > open, bar_index, 0)\nfloat h = ta.valuewhen(close > open, high, 0)[2]\nplot(b + h)"), vec![]);
}

#[test]
fn v5_number_to_bool() {
    let v5 = |body: &str| check(&parse(&format!("//@version=5\nindicator(\"t\")\n{body}")).unwrap()).diags.into_iter().filter(|d| d.is_error()).count();
    assert_eq!(v5("b = false\nb := close > open ? 1 : 0\nplot(close)"), 0);
    assert_eq!(codes("b = false\nb := close > open ? 1 : 0\nplot(close)"), vec![DiagCode::TypeMismatch]);
}

#[test]
fn int_division_type_left_open() {
    assert_eq!(codes("n = 100 / 4\nx = 0\nx := x * n\nplot(x)"), vec![]);
}

#[test]
fn typed_na() {
    assert_eq!(codes("x = na(float)\nx := close\nplot(x)"), vec![]);
}

#[test]
fn vwap_anchor_and_bands() {
    assert_eq!(codes("[v, u, l] = ta.vwap(hlc3, timeframe.change(\"D\"), 1)\nw = ta.vwap(close, barstate.isfirst)\nplot(v + w)"), vec![]);
}

#[test]
fn ternary_of_tuples() {
    assert_eq!(codes("f() => [1.0, true]\n[a, b] = close > open ? f() : [na, false]\nplot(a)"), vec![]);
}

#[test]
fn user_function_overloads() {
    assert_eq!(codes("g(a, b) => a + b\ng(a, b, c) => a + b + c\nplot(g(1, 2) + g(1, 2, 3))"), vec![]);
}

#[test]
fn v6_parameters_and_constants() {
    assert_eq!(codes("t = time()\nn = input.int(3, active = true)\nplot(close, linestyle = plot.linestyle_dashed, display = close > open ? display.all - display.status_line : display.none)"), vec![]);
}

#[test]
fn string_cast_and_trading_day() {
    assert_eq!(codes("s = string(na)\nplot(ta.change(time_tradingday))"), vec![]);
}

// ── Visual-only skip and causality taint (Task 8) ──

fn checked(body: &str) -> otw_script::check::Checked {
    check(&parse(&format!("//@version=6\nindicator(\"t\")\n{body}")).unwrap_or_else(|e| panic!("{e:?}")))
}
fn warned(c: &otw_script::check::Checked, feature: &str) -> bool {
    c.diags.iter().any(|d| d.code == DiagCode::SkippedVisual && d.args.first().map(String::as_str) == Some(feature))
}

#[test]
fn label_on_last_bar_runs() {
    let c = checked("if barstate.islast\n    label.new(bar_index, close, str.tostring(close))\nplot(close)");
    assert!(!c.has_errors(), "{:?}", c.diags);
    assert!(!warned(&c, "label.new"));
    assert!(c.dropped.is_empty());
    assert_eq!(c.meta.plots.len(), 1);
    assert!(c.meta.plots[0].causal);
}

#[test]
fn visual_variable_chain_skipped() {
    let c = checked("var label l = na\ntxt = \"v=\" + str.tostring(close)\nif barstate.islast\n    label.delete(l)\n    l := label.new(bar_index, high, txt)\nvar table t = table.new(position.top_right, 2, 2)\ntable.cell(t, 0, 0, txt)\nplot(close)");
    assert!(!c.has_errors(), "{:?}", c.diags);
}

#[test]
fn drawing_readback_runs() {
    let c = checked("l = label.new(bar_index, close)\nplot(label.get_x(l))");
    assert!(!c.has_errors(), "{:?}", c.diags);
}

#[test]
fn alertcondition_skipped() {
    let c = checked("up = ta.crossover(close, ta.sma(close, 5))\nalertcondition(up, \"Up\", \"msg\")\nif up\n    alert(\"x\")\nplot(close)");
    assert!(!c.has_errors(), "{:?}", c.diags);
    assert!(warned(&c, "alertcondition"));
}

#[test]
fn dead_code_with_unsupported_skipped() {
    let c = checked("arr = array.new<float>()\narray.push(arr, close)\nplot(close)");
    assert!(!c.has_errors(), "{:?}", c.diags);
}

#[test]
fn unsupported_feeding_a_plot_refused() {
    let c = checked("m = matrix.new<float>(2, 2)\nplot(matrix.rows(m))");
    assert!(c.has_errors());
}

#[test]
fn taint_data() {
    let c = checked("plot(close + last_bar_index)\nplot(close)");
    assert!(!c.meta.plots[0].causal);
    assert!(c.meta.plots[1].causal);
    let c = checked("x = barstate.islast ? 1.0 : 0.0\ny = x * 2\nplot(y)");
    assert!(!c.meta.plots[0].causal);
}

#[test]
fn taint_control() {
    let c = checked("x = 0.0\nif barstate.islast\n    x := 1\nplot(x)");
    assert!(!c.meta.plots[0].causal);
}

#[test]
fn negative_offset_noncausal() {
    let c = checked("plot(close, offset=-2)\nplot(close, offset=2)");
    assert!(!c.meta.plots[0].causal);
    assert!(c.meta.plots[1].causal);
}

#[test]
fn plot_meta_title_and_style() {
    let c = checked("plot(close, \"Close\", style=plot.style_histogram)\nplot(open)");
    assert_eq!(c.meta.plots[0].title, "Close");
    assert_eq!(c.meta.plots[0].style, otw_script::output::PlotStyle::Histogram);
    assert_eq!(c.meta.plots[1].title, "Plot");
}

#[test]
fn cosmetic_arg_skipped() {
    let c = checked("plot(close, editable=false)");
    assert!(!c.has_errors());
    assert!(warned(&c, "editable"));
}

#[test]
fn break_and_continue_of_a_kept_loop_are_kept() {
    let c = checked("i = 0\ns = 0\nwhile i < 10\n    i += 1\n    if i == 3\n        continue\n    if i == 6\n        break\n    s += i\nplot(s)");
    assert!(c.dropped.is_empty(), "{:?}", c.dropped);
}

#[test]
fn kept_assignment_keeps_its_declaration() {
    // `g` is only written, but the write sits in a kept block: its declaration must stay.
    let src = "//@version=6\nindicator(\"t\")\nbool g = true\nc = if close > open\n    color.green\nelse\n    g := false\n    color.red\nplot(close, color = c)";
    assert!(otw_script::compile(src).is_ok());
}

#[test]
fn function_used_before_definition_refused() {
    assert_eq!(codes("y = f(1)\nf(x) => x * 2\nplot(y)"), vec![DiagCode::UnknownName]);
}

// ── Final review fixes ──

#[test]
fn function_assigning_a_global_refused() {
    assert_eq!(codes("var g = 0\nf() =>\n    g := g + 1\n    0\nplot(f())"), vec![DiagCode::AssignGlobal]);
    assert_eq!(codes("f() =>\n    l = 0\n    l := 1\n    l\nplot(f())"), vec![], "locals are fine");
}

#[test]
fn non_literal_offset_noncausal() {
    let c = checked("len = input.int(3)\nplot(close, offset = -len)\nplot(close, offset = 0 - 2)\nplot(close, offset = 2)\nplot(close)");
    let causal: Vec<bool> = c.meta.plots.iter().map(|p| p.causal).collect();
    assert_eq!(causal, vec![false, false, true, true]);
}

#[test]
fn break_under_noncausal_condition_taints_loop() {
    let c = checked("cnt = 0\nfor i = 0 to 2\n    if barstate.islast\n        break\n    cnt += 1\nplot(cnt)");
    assert!(!c.meta.plots[0].causal);
}

#[test]
fn taint_names_root() {
    use otw_script::output::Taint;
    let t = |feature: &str, line: u32| Some(Taint { feature: feature.into(), line });
    let c = checked("x = barstate.islast ? 1 : 0\nplot(x)");
    assert_eq!(c.meta.plots[0].taint, t("barstate.islast", 3));
    let c = checked("plot(close + last_bar_index)");
    assert_eq!(c.meta.plots[0].taint, t("last_bar_index", 3));
    let c = checked("plot(close, offset = -2)");
    assert_eq!(c.meta.plots[0].taint, t("offset", 3));
    let c = checked("plot(ta.sma(close, 5))");
    assert_eq!(c.meta.plots[0].taint, None);
    assert!(c.meta.plots[0].causal);
    // Through a control condition and a function.
    let c = checked("f() => timenow\nx = 0.0\nif f() > 0\n    x := 1\nplot(x)");
    assert_eq!(c.meta.plots[0].taint, t("timenow", 3));
}
