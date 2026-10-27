# M05 snapshot bounds

The complete M03 image and event head come from one State job. Document reconciliation runs before that head is read. The image contains all eight existing collections. Serialization runs outside State. One permit bounds concurrent materialization and serialization. Each connection releases that permit before sending bytes.

The following bounds cover admitted state. They include JSON escaping at six output bytes per input UTF-8 byte, array separators, field names, and decimal counters. Fixed allowances exceed the fixed fields in the corresponding Rust types.

| Collection | Count | Serialized bytes per record | Source of bound |
| --- | ---: | ---: | --- |
| Tasks | 128 | 24,576 | `save` and `insert_task` reject larger serialized records. |
| Run summaries | 16 | 4,096 | UUIDs, enums, timestamps, and fixed accounting fields. Turns remain ordinary `Run` replies. |
| Inferences | 128 | 259,248 | Six times prompt 4,096, output 32,768, and request identifier 200, plus configuration 32,768 and fixed fields 4,096. |
| Inference configurations | 256 | 32,768 | Model 128, developer 200, three URLs of 1,024, sixfold escaping, and fixed fields. |
| Conversations | 256 | 128 | Two UUIDs and fixed fields. |
| Messages | 256 | 24,832 | Text 4,096 with sixfold escaping, two UUIDs, and fixed fields. |
| Documents | 256 | 51,200 | Markdown 4,096, eight source references of at most 512, sixfold escaping, and fixed fields 2,048. |
| Permissions | 256 | 128 | UUID, counter, boolean, and fixed fields. |

The sum is 64,317,440 bytes including 4,096 bytes for the reply and snapshot wrappers. The transfer limit is 83,886,080 bytes. A valid admitted snapshot fits without truncation or a permanent resource refusal. Run replies have an existing 1,048,576-byte bound. Artifact replies contain at most 8,388,608 binary bytes, expanded to 11,184,812 base64 bytes plus metadata. Other ordinary replies are bounded subsets of these collections. The same transfer format handles every reply.

The largest filesystem document read is 65,537 bytes including the overflow sentinel. A file larger than 16,384 bytes is rejected before parsing. Reconciliation stops before collecting a 257th document. Both checks happen during collection. A file that grows after its metadata check cannot cause an unbounded read. Directory iteration does not materialize unrelated filenames.

Decoded allocation has the following conservative per-record budgets. They include twice the owned string payload, twice the vector element storage, boxed values, and inline record storage.

| Collection | Bytes per decoded record |
| --- | ---: |
| Tasks | 262,144 |
| Run summaries | 4,096 |
| Inferences | 131,072 |
| Inference configurations | 16,384 |
| Conversations | 256 |
| Messages | 16,384 |
| Documents | 32,768 |
| Permissions | 128 |

Tasks are the only deeply nested snapshot records. Their complete serialized form is at most 24,576 bytes, including both copies of work arguments. Owned strings consume at most twice that allowance. A task has at most four effect entries and four outputs. Each of its two work specifications has at most twelve computer operations, four output declarations, or thirty-two process arguments and thirty-two environment entries. Allow 1,024 bytes per B-tree node and one node per environment entry. Two maps then consume at most 65,536 bytes in node storage. Add 8,192 for doubled effect storage, 2,048 for doubled output storage, 3,072 for computer operations, 2,048 for declarations, 2,048 for process boxes, and 8,192 for doubled task roots. The sum is 140,288 bytes, below the 262,144-byte budget. The rerunnable layout check verifies the Rust type sizes used by this calculation. No snapshot record is recursive.

Inference strings total at most 40,464 bytes before escaping. Twice that storage plus two 2,048-byte roots fits 131,072. Configuration strings total at most 3,400 bytes. Twice that plus two 1,024-byte roots fits 16,384. Messages use at most 4,096 text bytes and a 256-byte root. Documents use at most 8,192 string bytes, sixteen 64-byte source slots including spare capacity, and two 1,024-byte roots. Run summaries, conversations, and permissions have no variable payload. Their budgets cover doubled roots. Summing all record budgets gives 67,272,704 bytes.

The serializer first counts bytes through a capped writer, then allocates exactly that count and serializes the immutable reply. It never grows a partially filled maximum-size vector. The image and head remain one State result throughout both passes. Serialization scratch is bounded by one record. The State readers parse one database row at a time. Document reconciliation holds at most one extra encoded document or SQLite index row at a time.

Blocking artifact reads retain the same materialization permit until their work finishes, even if the requesting socket is cancelled. Rejected operations release the permit before waiting on a control queue. Cancellation cannot accumulate detached materializations.

Eight owner connections have at most one ordinary operation or transfer each. One global permit covers materialization and both serializer passes. Slow sends release it. Allow nine maximum transfer buffers to cover cancellation while a blocking serialization job finishes, the decoded image, and another 32 MiB for database rows, serializer scratch, socket frames, control queues, and document-index work. Additional owner recovery allocation stays below 1 GiB. This is an upper bound on recovery's allocations, not a claim about total process resident memory or measured performance.

The client has one transfer staging file and one atomically installed state file. Installation removes the transfer file before writing the replacement state staging file. Old state plus staging consumes at most 160 MiB, plus less than 1 MiB for credentials and metadata. Transfer deadlines and process startup remove interrupted staging files. The server creates no transfer files.

Run `cargo test -p botstral-manager --test m05_bounds -- --nocapture` to check the constants and layout assumptions. [Final corrected-source acceptance](m05-acceptance.md) demonstrated complete recovery and oversized ordinary replies. All required runtime gates passed. Parent evidence review and integration remain pending.
