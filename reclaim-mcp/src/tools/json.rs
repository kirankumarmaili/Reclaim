//! JSON tools: prettify, minify, semantic compare, validate. Pure functions on
//! inline strings; object keys compare order-insensitively, arrays by index.

use crate::server::ReclaimServer;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub enum Indent {
    Two,
    Four,
    Tab,
}

pub fn parse_indent(s: &str) -> Indent {
    match s {
        "4" => Indent::Four,
        "tab" => Indent::Tab,
        _ => Indent::Two,
    }
}

fn indent_bytes(i: &Indent) -> &'static [u8] {
    match i {
        Indent::Two => b"  ",
        Indent::Four => b"    ",
        Indent::Tab => b"\t",
    }
}

pub fn prettify(json: &str, indent: &Indent) -> Result<String, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent_bytes(indent));
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    use serde::Serialize as _;
    value.serialize(&mut ser).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

pub fn minify(json: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ValidateOut {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ParseError>,
}

pub fn validate(json: &str) -> ValidateOut {
    match serde_json::from_str::<Value>(json) {
        Ok(_) => ValidateOut { valid: true, error: None },
        Err(e) => ValidateOut {
            valid: false,
            error: Some(ParseError {
                message: e.to_string(),
                line: e.line(),
                column: e.column(),
            }),
        },
    }
}

#[derive(Debug, Serialize, JsonSchema, PartialEq)]
pub struct Diff {
    pub path: String,
    /// "added" | "removed" | "changed" | "type_changed"
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompareOut {
    pub equal: bool,
    pub diffs: Vec<Diff>,
}

fn type_tag(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Escape a JSON Pointer reference token (RFC 6901): ~ -> ~0, / -> ~1.
fn esc(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn diff_into(path: &str, l: &Value, r: &Value, out: &mut Vec<Diff>) {
    if type_tag(l) != type_tag(r) {
        out.push(Diff {
            path: path.to_string(),
            kind: "type_changed".into(),
            left: Some(l.clone()),
            right: Some(r.clone()),
        });
        return;
    }
    match (l, r) {
        (Value::Object(lo), Value::Object(ro)) => {
            for (k, lv) in lo {
                let child = format!("{path}/{}", esc(k));
                match ro.get(k) {
                    Some(rv) => diff_into(&child, lv, rv, out),
                    None => out.push(Diff {
                        path: child,
                        kind: "removed".into(),
                        left: Some(lv.clone()),
                        right: None,
                    }),
                }
            }
            for (k, rv) in ro {
                if !lo.contains_key(k) {
                    out.push(Diff {
                        path: format!("{path}/{}", esc(k)),
                        kind: "added".into(),
                        left: None,
                        right: Some(rv.clone()),
                    });
                }
            }
        }
        (Value::Array(la), Value::Array(ra)) => {
            let common = la.len().min(ra.len());
            for i in 0..common {
                diff_into(&format!("{path}/{i}"), &la[i], &ra[i], out);
            }
            for (i, lv) in la.iter().enumerate().skip(common) {
                out.push(Diff {
                    path: format!("{path}/{i}"),
                    kind: "removed".into(),
                    left: Some(lv.clone()),
                    right: None,
                });
            }
            for (i, rv) in ra.iter().enumerate().skip(common) {
                out.push(Diff {
                    path: format!("{path}/{i}"),
                    kind: "added".into(),
                    left: None,
                    right: Some(rv.clone()),
                });
            }
        }
        _ => {
            // Scalars of the same JSON type. Note: serde_json's Number equality is
            // representation-sensitive, so `1` vs `1.0` reports as `changed` (both
            // are type "number"). That is intentional — we surface representation
            // differences rather than silently treating them as equal.
            if l != r {
                out.push(Diff {
                    path: path.to_string(),
                    kind: "changed".into(),
                    left: Some(l.clone()),
                    right: Some(r.clone()),
                });
            }
        }
    }
}

pub fn compare(left: &str, right: &str) -> Result<CompareOut, String> {
    let l: Value = serde_json::from_str(left).map_err(|e| format!("left: {e}"))?;
    let r: Value = serde_json::from_str(right).map_err(|e| format!("right: {e}"))?;
    let mut diffs = Vec::new();
    diff_into("", &l, &r, &mut diffs);
    Ok(CompareOut { equal: diffs.is_empty(), diffs })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrettifyReq {
    pub json: String,
    /// "2" (default), "4", or "tab".
    pub indent: Option<String>,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct PrettifyResp {
    pub pretty: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct JsonReq {
    pub json: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct MinifyResp {
    pub minified: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompareReq {
    pub left: String,
    pub right: String,
}

#[tool_router(router = json_router, vis = "pub(crate)")]
impl ReclaimServer {
    #[tool(name = "json_prettify", description = "Pretty-print JSON with a chosen indent (2/4/tab).")]
    pub async fn json_prettify(
        &self,
        p: Parameters<PrettifyReq>,
    ) -> Result<Json<PrettifyResp>, String> {
        let indent = parse_indent(p.0.indent.as_deref().unwrap_or("2"));
        Ok(Json(PrettifyResp { pretty: prettify(&p.0.json, &indent)? }))
    }

    #[tool(name = "json_minify", description = "Minify JSON, stripping all insignificant whitespace.")]
    pub async fn json_minify(&self, p: Parameters<JsonReq>) -> Result<Json<MinifyResp>, String> {
        Ok(Json(MinifyResp { minified: minify(&p.0.json)? }))
    }

    #[tool(name = "json_validate", description = "Validate JSON; report parse errors with line/column.")]
    pub async fn json_validate(&self, p: Parameters<JsonReq>) -> Result<Json<ValidateOut>, String> {
        Ok(Json(validate(&p.0.json)))
    }

    #[tool(name = "json_compare", description = "Semantic diff of two JSON docs: object keys order-insensitive, arrays index-sensitive. Reports added/removed/changed/type_changed paths.")]
    pub async fn json_compare(&self, p: Parameters<CompareReq>) -> Result<Json<CompareOut>, String> {
        Ok(Json(compare(&p.0.left, &p.0.right)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettify_uses_requested_indent() {
        let out = prettify(r#"{"a":1}"#, &Indent::Four).unwrap();
        assert_eq!(out, "{\n    \"a\": 1\n}");
        let tabbed = prettify(r#"{"a":1}"#, &Indent::Tab).unwrap();
        assert_eq!(tabbed, "{\n\t\"a\": 1\n}");
    }

    #[test]
    fn prettify_rejects_invalid_json() {
        assert!(prettify("{nope}", &Indent::Two).is_err());
    }

    #[test]
    fn minify_strips_whitespace() {
        let out = minify("{\n  \"a\": [1, 2]\n}").unwrap();
        assert_eq!(out, r#"{"a":[1,2]}"#);
    }

    #[test]
    fn validate_reports_error_location() {
        let ok = validate(r#"{"a":1}"#);
        assert!(ok.valid && ok.error.is_none());
        let bad = validate("{\n  \"a\": }");
        assert!(!bad.valid);
        let e = bad.error.unwrap();
        assert!(e.line >= 1 && e.column >= 1);
    }

    #[test]
    fn compare_equal_objects_ignores_key_order() {
        let c = compare(r#"{"a":1,"b":2}"#, r#"{"b":2,"a":1}"#).unwrap();
        assert!(c.equal);
        assert!(c.diffs.is_empty());
    }

    #[test]
    fn compare_reports_added_removed_changed_and_typechange() {
        let c = compare(
            r#"{"keep":1,"gone":2,"num":3,"t":1}"#,
            r#"{"keep":1,"num":4,"t":"s","new":9}"#,
        )
        .unwrap();
        assert!(!c.equal);
        let kinds: std::collections::BTreeMap<String, String> = c
            .diffs
            .iter()
            .map(|d| (d.path.clone(), d.kind.clone()))
            .collect();
        assert_eq!(kinds.get("/gone").unwrap(), "removed");
        assert_eq!(kinds.get("/new").unwrap(), "added");
        assert_eq!(kinds.get("/num").unwrap(), "changed");
        assert_eq!(kinds.get("/t").unwrap(), "type_changed");
    }

    #[test]
    fn compare_arrays_are_index_sensitive() {
        let c = compare(r#"[1,2,3]"#, r#"[1,9,3]"#).unwrap();
        assert_eq!(c.diffs.len(), 1);
        assert_eq!(c.diffs[0].path, "/1");
        assert_eq!(c.diffs[0].kind, "changed");
    }

    #[test]
    fn json_router_lists_four_tools() {
        let names: Vec<String> = ReclaimServer::json_router()
            .list_all()
            .iter()
            .map(|t| t.name.to_string())
            .collect();
        for n in ["json_prettify", "json_minify", "json_compare", "json_validate"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }
}
