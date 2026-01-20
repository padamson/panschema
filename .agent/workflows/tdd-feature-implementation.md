---
description: How to implement a new feature using TDD
---

# TDD Feature Implementation Workflow

1.  **Understand the Goal**: Read the user request and identify the specific feature to implement.
2.  **Create/Update Implementation Plan**:
    -   Create a new plan or update existing one in `docs/implementation-plans/`.
    -   Define the new component/feature.
    -   **Identify the Test Level**:
        -   **Unit Tests**: Complex internal logic, edge cases (use `#[test]`).
        -   **Doctests**: Public API documentation and usage examples (use `/// \`\`\``).
        -   **Integration Tests**: CLI behavior, public API interaction (use `tests/*.rs`).
        -   **E2E Tests**: Validating generated documentation output (use `playwright-rs` to assert HTML correctness).
3.  **Write Tests First**:
    -   Choose the appropriate test level from above.
    -   Write the test case that asserts the desired behavior.
    -   Run the test to confirm it fails (`cargo nextest run` or `cargo test --doc`).
4.  **Implement Feature**:
    -   Write the minimal code to satisfy the test.
    -   Run the test again to confirm it passes.
5.  **Refactor**:
    -   Clean up the code.
    -   Ensure `cargo clippy` and `cargo fmt` pass.
6.  **Verify**:
    -   Run all tests to ensure no regressions.
    -   **Critical**: For HTML generation features, you *must* verify the output in a browser environment using the E2E tests.
