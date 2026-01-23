#!/bin/bash
set -e

INPUT_FILE="tests/fixtures/reference.ttl"
OUTPUT_DIR="target/doc-preview"

# Check for --watch flag
if [[ "$1" == "--watch" ]]; then
    if ! command -v cargo-watch &> /dev/null; then
        echo "Error: cargo-watch is not installed."
        echo "Please run: cargo install cargo-watch"
        exit 1
    fi

    echo "Mode: Hot Reload"
    echo "This will automatically rebuild and regenerate documentation when source code or input file changes."

    # 1. Start Server in Background
    echo "Starting preview server at http://localhost:3030 ..."
    cargo run --example preview &
    SERVER_PID=$!

    # Cleanup function
    cleanup() {
        echo "Stopping server..."
        kill $SERVER_PID
    }
    trap cleanup EXIT

    # 2. Start Watcher (Blocking)
    # Watch src/ directory and the input ontology file
    echo "Watching for changes..."
    cargo watch -w src -w "$INPUT_FILE" -x "run -- --input $INPUT_FILE --output $OUTPUT_DIR"

else
    # Standard Mode (Run once)
    echo "Generating documentation..."
    cargo run -- --input "$INPUT_FILE" --output "$OUTPUT_DIR"

    echo "Starting preview server..."
    cargo run --example preview
fi
