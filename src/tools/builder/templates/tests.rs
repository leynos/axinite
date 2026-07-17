//! Unit tests for the tool-builder template engine and templates.

use super::*;

#[test]
fn test_template_engine() {
    let mut engine = TemplateEngine::new();
    engine.set("name", "my_tool");
    engine.set("description", "A cool tool");

    let result = engine.render("Name: {{name}}, Desc: {{description}}");
    assert_eq!(result, "Name: my_tool, Desc: A cool tool");
}

#[test]
fn test_get_template() {
    let template = Template::get(TemplateType::WasmHttpTool);
    assert_eq!(template.name, "WASM HTTP Tool");
    assert!(!template.files.is_empty());
}

#[test]
fn test_render_no_variables() {
    let engine = TemplateEngine::new();
    let input = "Hello, world! No placeholders here.";
    assert_eq!(engine.render(input), input);
}

#[test]
fn test_render_variable_not_found() {
    let mut engine = TemplateEngine::new();
    engine.set("name", "axinite");
    let input = "Name: {{name}}, Missing: {{missing}}";
    assert_eq!(engine.render(input), "Name: axinite, Missing: {{missing}}");
}

#[test]
fn test_render_multiple_replacements_of_same_variable() {
    let mut engine = TemplateEngine::new();
    engine.set("x", "42");
    assert_eq!(engine.render("{{x}} + {{x}} = 2*{{x}}"), "42 + 42 = 2*42");
}

#[test]
fn test_set_overwrites_existing_variable() {
    let mut engine = TemplateEngine::new();
    engine.set("colour", "red");
    assert_eq!(engine.render("{{colour}}"), "red");
    engine.set("colour", "blue");
    assert_eq!(engine.render("{{colour}}"), "blue");
}

#[test]
fn test_render_template_all_files() {
    let mut engine = TemplateEngine::new();
    engine.set("name", "my_tool");
    engine.set("description", "does stuff");

    let template = Template::get(TemplateType::CliBinary);
    let rendered = engine.render_template(&template);

    assert_eq!(rendered.len(), template.files.len());
    // Paths should have variables substituted
    for (path, _content) in &rendered {
        assert!(!path.contains("{{name}}"));
    }
    // Content should have variables substituted
    for (_path, content) in &rendered {
        assert!(!content.contains("{{name}}"));
        assert!(!content.contains("{{description}}"));
    }
}

#[test]
fn test_all_template_types_return_non_empty() {
    let all_types = [
        TemplateType::WasmHttpTool,
        TemplateType::WasmTransformTool,
        TemplateType::WasmComputeTool,
        TemplateType::CliBinary,
        TemplateType::PythonScript,
        TemplateType::BashScript,
    ];
    for tt in all_types {
        let t = Template::get(tt);
        assert!(!t.name.is_empty(), "{:?} has empty name", tt);
        assert!(!t.description.is_empty(), "{:?} has empty description", tt);
        assert!(!t.files.is_empty(), "{:?} has no files", tt);
        for f in &t.files {
            assert!(
                !f.content.is_empty(),
                "{:?} file {:?} has empty content",
                tt,
                f.path
            );
        }
    }
}

#[test]
fn test_template_type_serde_roundtrip() {
    let all_types = [
        TemplateType::WasmHttpTool,
        TemplateType::WasmTransformTool,
        TemplateType::WasmComputeTool,
        TemplateType::CliBinary,
        TemplateType::PythonScript,
        TemplateType::BashScript,
    ];
    for tt in all_types {
        let json = serde_json::to_string(&tt).unwrap();
        let back: TemplateType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tt, "roundtrip failed for {:?} (json: {})", tt, json);
    }
}

#[test]
fn test_each_template_has_at_least_one_required_file() {
    let all_types = [
        TemplateType::WasmHttpTool,
        TemplateType::WasmTransformTool,
        TemplateType::WasmComputeTool,
        TemplateType::CliBinary,
        TemplateType::PythonScript,
        TemplateType::BashScript,
    ];
    for tt in all_types {
        let t = Template::get(tt);
        let required_count = t.files.iter().filter(|f| f.is_required).count();
        assert!(required_count >= 1, "{:?} has no required files", tt);
    }
}

#[test]
fn test_template_file_extensions() {
    // WASM and CLI templates should have Cargo.toml and .rs files
    for tt in [
        TemplateType::WasmHttpTool,
        TemplateType::WasmTransformTool,
        TemplateType::WasmComputeTool,
        TemplateType::CliBinary,
    ] {
        let t = Template::get(tt);
        let paths: Vec<&str> = t.files.iter().map(|f| f.path).collect();
        assert!(
            paths.iter().any(|p| p.ends_with("Cargo.toml")),
            "{:?} missing Cargo.toml",
            tt
        );
        assert!(
            paths.iter().any(|p| p.ends_with(".rs")),
            "{:?} missing .rs file",
            tt
        );
    }

    // Python template should have a .py file
    let py = Template::get(TemplateType::PythonScript);
    assert!(py.files.iter().any(|f| f.path.ends_with(".py")));

    // Bash template should have a .sh file
    let bash = Template::get(TemplateType::BashScript);
    assert!(bash.files.iter().any(|f| f.path.ends_with(".sh")));
}

#[test]
fn test_python_and_bash_templates_have_name_in_path() {
    let py = Template::get(TemplateType::PythonScript);
    assert!(
        py.files.iter().any(|f| f.path.contains("{{name}}")),
        "PythonScript template should have {{{{name}}}} in a file path"
    );

    let bash = Template::get(TemplateType::BashScript);
    assert!(
        bash.files.iter().any(|f| f.path.contains("{{name}}")),
        "BashScript template should have {{{{name}}}} in a file path"
    );
}
