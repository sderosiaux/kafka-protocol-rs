use failure::Error;
use git2::{Oid, Repository};
use std::fs;
use std::path::Path;
use std::process::Command;

pub mod generate_messages;

fn main() -> Result<(), Error> {
    // CARGO_MANIFEST_DIR is always set by cargo at compile time and
    // points at protocol_codegen/. Pre std::file!() canonicalization
    // was broken across release builds (file! returns the literal
    // pre-canonicalized path).
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_path = std::fs::canonicalize(manifest.join("../src/messages"))?;
    let messages_module_dir = output_path.to_str().unwrap();
    // Anchor the cloned kafka_repo next to protocol_codegen/Cargo.toml
    // so cwd-changes don't break this script.
    std::env::set_current_dir(manifest)?;

    // Download messages from head of Kafka repo
    let kafka_repo = Path::new("kafka_repo");
    let repo = if kafka_repo.exists() {
        println!("Fetching latest kafka repo");
        let repo = Repository::open(kafka_repo)?;
        repo.find_remote("origin")
            .unwrap()
            .fetch(&["trunk"], None, None)
            .unwrap();
        repo
    } else {
        println!("Cloning kafka repo");
        git2::build::RepoBuilder::new()
            .fetch_options(git2::FetchOptions::new())
            .with_checkout(git2::build::CheckoutBuilder::new())
            .clone("https://github.com/apache/kafka.git", kafka_repo)?
    };

    // Checkout the release commit
    // https://github.com/apache/kafka/releases/tag/4.1.0
    // checking out a tag with git2 is annoying -- we pin to the tag's commit sha instead
    // Kapture fork: bumped from 4.1.0 to apache/kafka trunk
    // 5e6150caf7 (2026-05-06) so schemas include TopicId on
    // OffsetCommit/Fetch v10, Enable2Pc on InitProducerId v6, Share
    // Groups, KRaft v2 additions, ApiVersions v5, etc.
    let release_commit = "5e6150caf7bde2e111c41f949b85c44c291a866c";
    println!("Checking out release {}", release_commit);
    let oid = Oid::from_str(release_commit).unwrap();
    let commit = repo
        .find_commit(oid)
        .expect("Could not find release commit!")
        .into_object();
    repo.checkout_tree(&commit, None).unwrap();
    repo.set_head_detached(commit.id()).unwrap();

    // Clear output directory
    for file in fs::read_dir(messages_module_dir)? {
        let file = file?;
        if file.file_type()?.is_file() {
            let path = file.path();
            if path.extension() == Some("rs".as_ref()) {
                fs::remove_file(path)?;
            }
        }
    }

    // Find input files
    let mut input_file_paths = Vec::new();
    for file in fs::read_dir(kafka_repo.join("clients/src/main/resources/common/message"))? {
        let file = file?;
        if file.file_type()?.is_file() {
            let path = file.path();
            if path.extension() == Some("json".as_ref()) {
                input_file_paths.push(path);
            }
        }
    }

    generate_messages::run(messages_module_dir, input_file_paths)?;

    println!("Running cargo fmt...");
    let mut process = Command::new("cargo")
        .args(vec!["fmt"])
        .spawn()
        .expect("cargo fmt failed");

    process.wait().expect("cargo fmt failed");

    Ok(())
}
