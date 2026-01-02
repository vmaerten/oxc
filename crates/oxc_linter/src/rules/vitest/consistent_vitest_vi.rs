use oxc_ast::{AstKind, ast::ImportDeclarationSpecifier};
use oxc_diagnostics::OxcDiagnostic;
use oxc_macros::declare_oxc_lint;
use oxc_span::{GetSpan, Span};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{context::LintContext, rule::Rule, utils::PossibleJestNode};

fn consistent_vitest_vi_diagnostic(span: Span, disallowed: &str, preferred: &str) -> OxcDiagnostic {
    OxcDiagnostic::warn(format!("Use `{preferred}` instead of `{disallowed}`"))
        .with_help(format!("Replace `{disallowed}` with `{preferred}`"))
        .with_label(span)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, JsonSchema, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UtilName {
    #[default]
    Vi,
    Vitest,
}

impl UtilName {
    fn as_str(self) -> &'static str {
        match self {
            Self::Vi => "vi",
            Self::Vitest => "vitest",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Vi => Self::Vitest,
            Self::Vitest => Self::Vi,
        }
    }
}

#[derive(Debug, Clone, Default, JsonSchema, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConsistentVitestViConfig {
    #[serde(rename = "fn")]
    fn_name: UtilName,
}

#[derive(Debug, Default, Clone)]
pub struct ConsistentVitestVi(Box<ConsistentVitestViConfig>);

impl std::ops::Deref for ConsistentVitestVi {
    type Target = ConsistentVitestViConfig;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

declare_oxc_lint!(
    /// ### What it does
    ///
    /// Enforces consistent usage of `vi` or `vitest` for Vitest utility functions.
    ///
    /// ### Why is this bad?
    ///
    /// Mixing `vi` and `vitest` in the same codebase leads to inconsistency and
    /// can confuse developers about which one to use. Both `vi` and `vitest` are
    /// aliases for the same utility object, so using one consistently improves
    /// code readability.
    ///
    /// ### Examples
    ///
    /// Examples of **incorrect** code for this rule (with default `{ "fn": "vi" }`):
    /// ```javascript
    /// import { vitest } from "vitest";
    /// vitest.mock("./module");
    /// ```
    ///
    /// Examples of **correct** code for this rule:
    /// ```javascript
    /// import { vi } from "vitest";
    /// vi.mock("./module");
    /// ```
    ConsistentVitestVi,
    vitest,
    style,
    fix,
    config = ConsistentVitestViConfig,
);

impl ConsistentVitestVi {
    fn preferred(&self) -> &'static str {
        self.fn_name.as_str()
    }

    fn disallowed(&self) -> &'static str {
        self.fn_name.other().as_str()
    }
}

impl Rule for ConsistentVitestVi {
    fn from_configuration(value: serde_json::Value) -> Result<Self, serde_json::error::Error> {
        let config = value
            .get(0)
            .cloned()
            .map(serde_json::from_value::<ConsistentVitestViConfig>)
            .transpose()?
            .unwrap_or_default();

        Ok(Self(Box::new(config)))
    }

    fn run<'a>(&self, node: &oxc_semantic::AstNode<'a>, ctx: &LintContext<'a>) {
        let AstKind::ImportDeclaration(import_decl) = node.kind() else {
            return;
        };

        if import_decl.source.value.as_str() != "vitest" {
            return;
        }

        let Some(specifiers) = &import_decl.specifiers else {
            return;
        };

        let disallowed = self.disallowed();
        let preferred = self.preferred();

        for specifier in specifiers {
            let ImportDeclarationSpecifier::ImportSpecifier(spec) = specifier else {
                continue;
            };

            let imported_name = spec.imported.name();
            let local_name = spec.local.name.as_str();

            if imported_name == disallowed && local_name == disallowed {
                ctx.diagnostic_with_fix(
                    consistent_vitest_vi_diagnostic(spec.imported.span(), disallowed, preferred),
                    |fixer| fixer.replace(spec.imported.span(), preferred),
                );
            }
        }
    }

    fn run_on_jest_node<'a, 'c>(
        &self,
        jest_node: &PossibleJestNode<'a, 'c>,
        ctx: &'c LintContext<'a>,
    ) {
        let AstKind::CallExpression(call_expr) = jest_node.node.kind() else {
            return;
        };

        let Some(member_expr) = call_expr.callee.as_member_expression() else {
            return;
        };

        let Some(ident) = member_expr.object().get_identifier_reference() else {
            return;
        };

        let disallowed = self.disallowed();
        let preferred = self.preferred();
        let name_to_check = jest_node.original.unwrap_or(ident.name.as_str());

        if name_to_check == disallowed {
            ctx.diagnostic_with_fix(
                consistent_vitest_vi_diagnostic(ident.span, disallowed, preferred),
                |fixer| fixer.replace(ident.span, preferred),
            );
        }
    }
}

#[test]
fn test() {
    use crate::tester::Tester;

    let pass = vec![
        (r#"import { expect, it } from "vitest";"#, None),
        (r#"import { vi } from "vitest";"#, None),
        (r#"import { vitest } from "vitest";"#, Some(serde_json::json!([{ "fn": "vitest" }]))),
        (
            r#"import { vi } from "vitest";
            vi.stubEnv("NODE_ENV", "production");"#,
            None,
        ),
        (r#"vi.stubEnv("NODE_ENV", "production");"#, None),
        // Aliased imports should be allowed
        (r#"import { vitest as vt } from "vitest";"#, None),
        (r#"import { vi as v } from "vitest";"#, Some(serde_json::json!([{ "fn": "vitest" }]))),
        // vitest config - vitest usage is correct
        (
            r#"import { vitest } from "vitest";
            vitest.stubEnv("NODE_ENV", "production");"#,
            Some(serde_json::json!([{ "fn": "vitest" }])),
        ),
    ];

    let fail = vec![
        (r#"import { vitest } from "vitest";"#, None),
        (r#"import { expect, vi, vitest } from "vitest";"#, None),
        (
            r#"import { vitest } from "vitest";
            vitest.stubEnv("NODE_ENV", "production");"#,
            None,
        ),
        (
            r#"vi.stubEnv("NODE_ENV", "production");
            vi.clearAllMocks();"#,
            Some(serde_json::json!([{ "fn": "vitest" }])),
        ),
        // vi import when vitest is preferred
        (r#"import { vi } from "vitest";"#, Some(serde_json::json!([{ "fn": "vitest" }]))),
    ];

    let fix = vec![
        (r#"import { vitest } from "vitest";"#, r#"import { vi } from "vitest";"#, None),
        (
            r#"import { expect, vi, vitest } from "vitest";"#,
            r#"import { expect, vi, vi } from "vitest";"#,
            None,
        ),
        (
            r#"import { vitest } from "vitest";
            vitest.stubEnv("NODE_ENV", "production");"#,
            r#"import { vi } from "vitest";
            vi.stubEnv("NODE_ENV", "production");"#,
            None,
        ),
        (
            r#"vi.stubEnv("NODE_ENV", "production");
            vi.clearAllMocks();"#,
            r#"vitest.stubEnv("NODE_ENV", "production");
            vitest.clearAllMocks();"#,
            Some(serde_json::json!([{ "fn": "vitest" }])),
        ),
    ];

    Tester::new(ConsistentVitestVi::NAME, ConsistentVitestVi::PLUGIN, pass, fail)
        .with_vitest_plugin(true)
        .expect_fix(fix)
        .test_and_snapshot();
}
