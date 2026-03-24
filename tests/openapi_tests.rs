//! OpenAPI integration tests.
//!
//! Tests cover: JSON/YAML spec loading, $ref resolution (simple, nested, circular),
//! command extraction from a Petstore-like spec, and execution with mocked HTTP backend.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use mcp2cli::core::types::{ParamLocation, ParamType};
use mcp2cli::openapi::commands::extract_openapi_commands;
use mcp2cli::openapi::executor::execute_openapi;
use mcp2cli::openapi::refs::resolve_refs;
use mcp2cli::openapi::spec::load_spec;

// ---------------------------------------------------------------------------
// Spec loading tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_load_json_spec_from_file() {
    let spec = load_spec("tests/fixtures/petstore.json", &[])
        .await
        .unwrap();
    assert_eq!(spec["info"]["title"], "Petstore");
    assert!(spec["paths"]["/pets"].is_object());
    assert!(spec["paths"]["/pets/{petId}"].is_object());
}

#[tokio::test]
async fn test_load_yaml_spec_from_file() {
    let spec = load_spec("tests/fixtures/petstore.yaml", &[])
        .await
        .unwrap();
    assert_eq!(spec["info"]["title"], "Petstore YAML");
    assert!(spec["paths"]["/pets"].is_object());
    assert!(spec["paths"]["/pets/{petId}"].is_object());
}

#[tokio::test]
async fn test_load_json_spec_from_url() {
    let spec_json = std::fs::read_to_string("tests/fixtures/petstore.json").unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/spec.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(&spec_json)
                .insert_header("Content-Type", "application/json"),
        )
        .mount(&server)
        .await;

    let spec = load_spec(&format!("{}/spec.json", server.uri()), &[])
        .await
        .unwrap();
    assert_eq!(spec["info"]["title"], "Petstore");
}

#[tokio::test]
async fn test_load_yaml_spec_from_url() {
    let spec_yaml = std::fs::read_to_string("tests/fixtures/petstore.yaml").unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/spec.yaml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(&spec_yaml)
                .insert_header("Content-Type", "text/yaml"),
        )
        .mount(&server)
        .await;

    let spec = load_spec(&format!("{}/spec.yaml", server.uri()), &[])
        .await
        .unwrap();
    assert_eq!(spec["info"]["title"], "Petstore YAML");
}

// ---------------------------------------------------------------------------
// $ref resolution tests
// ---------------------------------------------------------------------------

#[test]
fn test_simple_ref_resolution() {
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/petstore_refs.json").unwrap(),
    )
    .unwrap();
    let mut seen = HashSet::new();
    let resolved = resolve_refs(&spec, &spec, &mut seen);

    // The LimitParam $ref in listPets should be resolved to an inline param
    let list_params = &resolved["paths"]["/pets"]["get"]["parameters"];
    let first = &list_params[0];
    assert_eq!(first["name"], "limit");
    assert_eq!(first["in"], "query");
}

#[test]
fn test_nested_ref_resolution() {
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/petstore_refs.json").unwrap(),
    )
    .unwrap();
    let mut seen = HashSet::new();
    let resolved = resolve_refs(&spec, &spec, &mut seen);

    // Pet -> owner -> Owner should be fully resolved
    let pet_schema =
        &resolved["paths"]["/pets"]["post"]["requestBody"]["content"]["application/json"]["schema"];
    let owner = &pet_schema["properties"]["owner"];
    assert_eq!(owner["type"], "object");
    assert!(owner["properties"]["name"].is_object());
    assert!(owner["properties"]["email"].is_object());
}

#[test]
fn test_circular_ref_detection() {
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/petstore_refs.json").unwrap(),
    )
    .unwrap();
    let node_schema = &spec["components"]["schemas"]["Node"];
    let mut seen = HashSet::new();
    let resolved = resolve_refs(node_schema, &spec, &mut seen);

    // The circular child should be resolved to an empty object
    let child = &resolved["properties"]["child"];
    assert!(child.is_object());
    // The circular reference produces an empty object (no infinite recursion)
    assert!(child.get("properties").is_none() || child["properties"]["child"] == json!({}));
}

#[test]
fn test_ref_reuse_in_sibling_positions() {
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/petstore_refs.json").unwrap(),
    )
    .unwrap();
    let mut seen = HashSet::new();
    let resolved = resolve_refs(&spec, &spec, &mut seen);

    // PetIdParam is used in /pets/{petId} path-level params
    let pet_id_param = &resolved["paths"]["/pets/{petId}"]["parameters"][0];
    assert_eq!(pet_id_param["name"], "petId");
    assert_eq!(pet_id_param["in"], "path");
}

// ---------------------------------------------------------------------------
// Command extraction tests
// ---------------------------------------------------------------------------

#[test]
fn test_command_extraction_from_json_spec() {
    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);

    // Should have 6 operations: listPets, createPet, getPetById, updatePet, deletePet, uploadPetPhoto
    assert_eq!(cmds.len(), 6);
}

#[test]
fn test_command_names_from_operation_id() {
    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();

    assert!(
        names.contains(&"list-pets"),
        "Missing list-pets: {:?}",
        names
    );
    assert!(
        names.contains(&"create-pet"),
        "Missing create-pet: {:?}",
        names
    );
    assert!(
        names.contains(&"get-pet-by-id"),
        "Missing get-pet-by-id: {:?}",
        names
    );
    assert!(
        names.contains(&"update-pet"),
        "Missing update-pet: {:?}",
        names
    );
    assert!(
        names.contains(&"delete-pet"),
        "Missing delete-pet: {:?}",
        names
    );
    assert!(
        names.contains(&"upload-pet-photo"),
        "Missing upload-pet-photo: {:?}",
        names
    );
}

#[test]
fn test_command_params_locations() {
    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);

    // listPets: 2 query params (limit, status)
    let list = cmds.iter().find(|c| c.name == "list-pets").unwrap();
    assert_eq!(list.params.len(), 2);
    assert!(list
        .params
        .iter()
        .all(|p| p.location == ParamLocation::Query));

    // getPetById: 1 path param from path-level
    let get = cmds.iter().find(|c| c.name == "get-pet-by-id").unwrap();
    assert_eq!(get.params.len(), 1);
    assert_eq!(get.params[0].location, ParamLocation::Path);
    assert_eq!(get.params[0].original_name, "petId");
    assert!(get.params[0].required);

    // updatePet: path param + header param + 2 body params
    let update = cmds.iter().find(|c| c.name == "update-pet").unwrap();
    let path_params: Vec<_> = update
        .params
        .iter()
        .filter(|p| p.location == ParamLocation::Path)
        .collect();
    let header_params: Vec<_> = update
        .params
        .iter()
        .filter(|p| p.location == ParamLocation::Header)
        .collect();
    let body_params: Vec<_> = update
        .params
        .iter()
        .filter(|p| p.location == ParamLocation::Body)
        .collect();
    assert_eq!(path_params.len(), 1);
    assert_eq!(header_params.len(), 1);
    assert_eq!(header_params[0].original_name, "X-Request-Id");
    assert_eq!(body_params.len(), 2);
}

#[test]
fn test_multipart_file_param_detection() {
    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let upload = cmds.iter().find(|c| c.name == "upload-pet-photo").unwrap();

    assert_eq!(upload.content_type.as_deref(), Some("multipart/form-data"));
    let file_param = upload
        .params
        .iter()
        .find(|p| p.original_name == "file")
        .unwrap();
    assert_eq!(file_param.location, ParamLocation::File);

    let desc_param = upload
        .params
        .iter()
        .find(|p| p.original_name == "description")
        .unwrap();
    assert_eq!(desc_param.location, ParamLocation::Body);
}

#[test]
fn test_enum_choices_in_extracted_commands() {
    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let list = cmds.iter().find(|c| c.name == "list-pets").unwrap();
    let status = list
        .params
        .iter()
        .find(|p| p.original_name == "status")
        .unwrap();
    assert_eq!(
        status.choices.as_ref().unwrap(),
        &["available", "pending", "sold"]
    );
}

#[test]
fn test_command_extraction_after_ref_resolution() {
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/petstore_refs.json").unwrap(),
    )
    .unwrap();
    let mut seen = HashSet::new();
    let resolved = resolve_refs(&spec, &spec, &mut seen);
    let cmds = extract_openapi_commands(&resolved);

    // listPets should have the resolved limit param
    let list = cmds.iter().find(|c| c.name == "list-pets").unwrap();
    assert_eq!(list.params.len(), 1);
    assert_eq!(list.params[0].original_name, "limit");
    assert_eq!(list.params[0].location, ParamLocation::Query);
    assert_eq!(list.params[0].rust_type, ParamType::Int);

    // createPet should have resolved Pet schema body params (name, tag, owner)
    let create = cmds.iter().find(|c| c.name == "create-pet").unwrap();
    assert!(create.has_body);
    let param_names: Vec<&str> = create
        .params
        .iter()
        .map(|p| p.original_name.as_str())
        .collect();
    assert!(param_names.contains(&"name"));
    assert!(param_names.contains(&"tag"));
    // owner is a resolved nested object
    assert!(param_names.contains(&"owner"));

    // getPetById should have resolved PetIdParam
    let get = cmds.iter().find(|c| c.name == "get-pet-by-id").unwrap();
    assert_eq!(get.params.len(), 1);
    assert_eq!(get.params[0].original_name, "petId");
    assert_eq!(get.params[0].location, ParamLocation::Path);
}

// ---------------------------------------------------------------------------
// Execution tests with wiremock
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_execute_get_with_path_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pets/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 42,
            "name": "Fido",
            "tag": "dog"
        })))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let get_cmd = cmds.iter().find(|c| c.name == "get-pet-by-id").unwrap();

    let mut args = HashMap::new();
    args.insert("petId".to_string(), json!(42));

    let result = execute_openapi(get_cmd, &args, &server.uri(), &HashMap::new())
        .await
        .unwrap();
    assert_eq!(result["name"], "Fido");
    assert_eq!(result["id"], 42);
}

#[tokio::test]
async fn test_execute_get_with_query_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pets"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "name": "Fido"},
            {"id": 2, "name": "Rex"}
        ])))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let list_cmd = cmds.iter().find(|c| c.name == "list-pets").unwrap();

    let mut args = HashMap::new();
    args.insert("limit".to_string(), json!(10));
    args.insert("status".to_string(), json!("available"));

    let result = execute_openapi(list_cmd, &args, &server.uri(), &HashMap::new())
        .await
        .unwrap();
    assert!(result.is_array());
    assert_eq!(result.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_execute_post_with_json_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/pets"))
        .and(body_json(json!({"name": "Luna", "tag": "cat"})))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 99,
            "name": "Luna",
            "tag": "cat"
        })))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let create_cmd = cmds.iter().find(|c| c.name == "create-pet").unwrap();

    let mut args = HashMap::new();
    args.insert("name".to_string(), json!("Luna"));
    args.insert("tag".to_string(), json!("cat"));

    let result = execute_openapi(create_cmd, &args, &server.uri(), &HashMap::new())
        .await
        .unwrap();
    assert_eq!(result["id"], 99);
    assert_eq!(result["name"], "Luna");
}

#[tokio::test]
async fn test_execute_put_with_header_params() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/pets/5"))
        .and(header("X-Request-Id", "req-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 5,
            "name": "Updated"
        })))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let update_cmd = cmds.iter().find(|c| c.name == "update-pet").unwrap();

    let mut args = HashMap::new();
    args.insert("petId".to_string(), json!(5));
    args.insert("X-Request-Id".to_string(), json!("req-123"));
    args.insert("name".to_string(), json!("Updated"));

    let result = execute_openapi(update_cmd, &args, &server.uri(), &HashMap::new())
        .await
        .unwrap();
    assert_eq!(result["name"], "Updated");
}

#[tokio::test]
async fn test_execute_delete_request() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/pets/7"))
        .respond_with(ResponseTemplate::new(204).set_body_string(""))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let delete_cmd = cmds.iter().find(|c| c.name == "delete-pet").unwrap();

    let mut args = HashMap::new();
    args.insert("petId".to_string(), json!(7));

    let result = execute_openapi(delete_cmd, &args, &server.uri(), &HashMap::new())
        .await
        .unwrap();
    // Empty response body becomes empty string
    assert!(result.is_string() || result.is_null());
}

#[tokio::test]
async fn test_execute_with_auth_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pets"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let list_cmd = cmds.iter().find(|c| c.name == "list-pets").unwrap();

    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer test-token".to_string());

    let result = execute_openapi(list_cmd, &HashMap::new(), &server.uri(), &headers)
        .await
        .unwrap();
    assert!(result.is_array());
}

#[tokio::test]
async fn test_execute_non_2xx_returns_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/pets/999"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"error": "Pet not found"})))
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let get_cmd = cmds.iter().find(|c| c.name == "get-pet-by-id").unwrap();

    let mut args = HashMap::new();
    args.insert("petId".to_string(), json!(999));

    let result = execute_openapi(get_cmd, &args, &server.uri(), &HashMap::new()).await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("404"),
        "Error should mention 404: {err_msg}"
    );
}

#[tokio::test]
async fn test_execute_multipart_upload() {
    let server = MockServer::start().await;
    // Multipart requests are harder to match exactly, so just match method + path
    Mock::given(method("POST"))
        .and(path("/pets/10/photo"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"photoUrl": "https://example.com/photo.jpg"})),
        )
        .mount(&server)
        .await;

    let spec: Value =
        serde_json::from_str(&std::fs::read_to_string("tests/fixtures/petstore.json").unwrap())
            .unwrap();
    let cmds = extract_openapi_commands(&spec);
    let upload_cmd = cmds.iter().find(|c| c.name == "upload-pet-photo").unwrap();

    // Create a temp file to upload
    let tmp_dir = tempfile::tempdir().unwrap();
    let tmp_file = tmp_dir.path().join("test_photo.png");
    std::fs::write(&tmp_file, b"fake png data").unwrap();

    let mut args = HashMap::new();
    args.insert("petId".to_string(), json!(10));
    args.insert("file".to_string(), json!(tmp_file.to_str().unwrap()));
    args.insert("description".to_string(), json!("A cute pet photo"));

    let result = execute_openapi(upload_cmd, &args, &server.uri(), &HashMap::new())
        .await
        .unwrap();
    assert_eq!(result["photoUrl"], "https://example.com/photo.jpg");
}

// ---------------------------------------------------------------------------
// Remote spec loading with custom headers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_load_spec_with_auth_headers() {
    let spec_json = std::fs::read_to_string("tests/fixtures/petstore.json").unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/spec"))
        .and(header("X-API-Key", "secret-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(&spec_json)
                .insert_header("Content-Type", "application/json"),
        )
        .mount(&server)
        .await;

    let headers = vec![("X-API-Key".to_string(), "secret-key".to_string())];
    let spec = load_spec(&format!("{}/spec", server.uri()), &headers)
        .await
        .unwrap();
    assert_eq!(spec["info"]["title"], "Petstore");
}
