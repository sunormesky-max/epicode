use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let args: Vec<String> = std::env::args().collect();
    let data_dir = args.get(1).map(|s| s.as_str()).unwrap_or("/var/lib/epicode");
    let data_path = PathBuf::from(data_dir);

    let model_dir = {
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("models"),
            PathBuf::from("/opt/epicode/models"),
        ];
        candidates.into_iter()
            .find(|d| d.join("model.onnx").exists())
            .unwrap_or_else(|| PathBuf::from("models"))
    };

    println!("Loading VectorLayer from {}...", model_dir.display());
    let vector = match epicode::engine::vector::VectorLayer::load(&model_dir) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("FATAL: cannot load VectorLayer: {}", e);
            std::process::exit(1);
        }
    };
    println!("VectorLayer loaded ({} dims)", epicode::engine::vector::EMBEDDING_DIM);

    let users_dir = data_path.join("users");
    if !users_dir.exists() {
        println!("No users directory found at {}", users_dir.display());
        return;
    }

    let entries = match std::fs::read_dir(&users_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Cannot read users dir: {}", e);
            std::process::exit(1);
        }
    };

    let mut total_migrated = 0usize;
    let mut total_users = 0usize;

    for entry in entries.flatten() {
        let user_id = entry.file_name().to_string_lossy().to_string();
        let db_path = entry.path().join("tetramem.db");
        if !db_path.exists() {
            continue;
        }

        total_users += 1;
        print!("[{}] ", user_id);

        let conn = match rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        ) {
            Ok(c) => c,
            Err(e) => {
                println!("SKIP (cannot open db: {})", e);
                continue;
            }
        };

        let mut stmt = match conn.prepare("SELECT id, content FROM tetrahedrons ORDER BY id") {
            Ok(s) => s,
            Err(e) => {
                println!("SKIP (cannot prepare: {})", e);
                continue;
            }
        };

        let rows: Vec<(u64, String)> = match stmt.query_map([], |row| {
            let id: u64 = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((id, content))
        }) {
            Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                println!("SKIP (query failed: {})", e);
                continue;
            }
        };

        drop(stmt);

        if rows.is_empty() {
            println!("0 memories (skipped)");
            continue;
        }

        let mut updated = 0usize;
        let mut failed = 0usize;

        for (id, content) in &rows {
            match vector.embed(content) {
                Ok(embedding) => {
                    let blob = epicode::engine::vector::VectorLayer::embedding_to_blob(&embedding);
                    match conn.execute(
                        "UPDATE tetrahedrons SET embedding = ?1 WHERE id = ?2",
                        rusqlite::params![&blob[..], id],
                    ) {
                        Ok(_) => updated += 1,
                        Err(e) => {
                            eprintln!("  update failed for {}: {}", id, e);
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  embed failed for {}: {}", id, e);
                    failed += 1;
                }
            }
        }

        drop(conn);
        total_migrated += updated;
        println!("{} migrated, {} failed", updated, failed);
    }

    println!("\n=== Migration Complete ===");
    println!("Users: {}", total_users);
    println!("Total memories migrated: {}", total_migrated);
}
