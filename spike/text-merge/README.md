# text-merge spike (ADR-012, validation item 3)

Token-level, block-aware three-way merge for Markdown text.

    cargo test --release                        # unit tests + properties + 30k-case campaign
    cargo run --release --example campaign 100000
    UNGATED=1 cargo run --release --example campaign   # without the verification gate

Results and limits: ADR-012, "Text merge results".
