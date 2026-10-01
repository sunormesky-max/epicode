# Local audit reproduction results

Source commit: `35d3e0566dffa4cbd3fd1b9556b504f9b8631f59`

Run from `backend` on Windows:

```text
cargo run --locked --offline --features ort/load-dynamic --example audit_probe
```

Exit status: 0. The probe uses disposable accounts and a loopback TCP listener. Output below excludes build progress and the machine-specific temporary directory. It demonstrates current defects; it is not a passing security regression suite. It does not exercise the HTTP server or an encrypted restart.

```jsonl
{"actual_user_id":"mcp-default","expected_user_id":"audit-viewer","probe":"engine_identity_without_shared_vector"}
{"probe":"viewer_mcp_update","protocol_ok":true,"response":{"data":{"ctx":{"t":"16:25:42","Δ":"idle","◆":"ready"},"fields_updated":["content"],"id":1,"side_effects":{"cluster_changed":false,"label_index_updated":false},"status":"updated"},"protocol":{"error":null,"ok":true,"schema_version":"1.0","tool":"memory_update"},"status":{"identity":{"name":"Audit fixture","system":"Epicode"},"space":{"energy":9990.0,"memories":1}}},"role_allows_write":false}
{"old_key_valid":false,"probe":"tcp_read_after_key_reset","protocol_ok":true,"response":{"data":{"aliases":[],"content":"Local audit record modified by viewer","ctx":{"t":"16:25:43","Δ":"idle","◆":"ready"},"id":1,"labels":["audit-fixture"],"metrics":{"access_count":0,"embedding_dims":0,"importance":0.8,"memory_type":"general","rationale":null,"valid_from":1790843125,"valid_to":null},"relations_summary":{"degree":0,"strongest":null,"type_distribution":{}},"similarity":1.0,"source":["label"],"tier":"primary","timestamp":1790843125,"topology":{"cluster_id":0,"cluster_size":1,"is_hub":false}},"protocol":{"error":null,"ok":true,"schema_version":"1.0","tool":"memory_get"},"status":{"identity":{"name":"Audit fixture","system":"Epicode"},"space":{"energy":9990.0,"memories":1}}}}
{"before_newline":"connection_still_open","bytes_sent":1049600,"closed_after_newline":true,"probe":"tcp_limit_before_newline"}
```

Role unit tests: `cargo test --locked --offline --features ort/load-dynamic --lib user_manager -- --test-threads=1` — 4 passed, 0 failed. This feature selection avoids the local static ONNX link conflict; it does not validate the default production link configuration.
