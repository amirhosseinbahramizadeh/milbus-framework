# Contributing to MILBUS Framework

<p align="center">
  <strong>MILBUS FRAMEWORK</strong>
  <br/>
  <sub>CONTRIBUTOR OPERATIONS & DEVELOPMENT STANDARDS</sub>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/CONTRIBUTIONS-WELCOME-4B5320?style=for-the-badge" alt="Contributions Welcome"/>
  <img src="https://img.shields.io/badge/LANGUAGE-RUST-B7410E?style=for-the-badge&logo=rust&logoColor=white" alt="Rust"/>
  <img src="https://img.shields.io/badge/QUALITY-TESTED-202020?style=for-the-badge" alt="Quality Tested"/>
</p>

---

## 01 / Mission

Thank you for your interest in contributing to **MILBUS Framework**.

MILBUS Framework is an open-source Rust project focused on avionics protocol simulation, virtual communication environments, protocol analysis, and controlled fault-injection testing.

We welcome contributions that improve correctness, reliability, maintainability, documentation, testing, and the extensibility of the framework.

This guide describes the recommended workflow for proposing changes and maintaining consistent engineering standards across the project.

## 02 / Contribution Areas

Contributions are welcome in the following areas:

| Area | Examples |
|---|---|
| Protocol implementations | MIL-STD-1553B, ARINC 429, Link 16 |
| Simulation infrastructure | Virtual buses, channels, device roles, simulation lifecycle |
| Protocol analysis | Frame validation, parsing, inspection, and reporting |
| Testing | Unit tests, integration tests, regression tests, fixtures |
| CLI and TUI | Command-line workflows, terminal interfaces, usability |
| Reliability | Error handling, input validation, panic prevention |
| Performance | Benchmarks, allocation reduction, efficient message handling |
| Documentation | Architecture guides, examples, protocol notes |
| Build and CI | Cargo workflows, GitHub Actions, dependency checks |
| Security | Responsible vulnerability handling and defensive improvements |

Please verify the current implementation and module structure before proposing changes based on a particular component.

## 03 / Before You Start

Before opening an issue or pull request:

1. Check existing issues and pull requests for related work.
2. Review the README and relevant source modules.
3. Identify the expected behavior and the problem being addressed.
4. For substantial changes, open an issue or discussion first.
5. For security vulnerabilities, follow `SECURITY.md` instead of publicly disclosing sensitive details.

Small documentation fixes and isolated bug fixes generally do not require prior approval.

## 04 / Development Environment

### Prerequisites

- A supported Rust toolchain
- Cargo
- Git
- A terminal compatible with the project's CLI and, where applicable, TUI

Install Rust using the official instructions at https://www.rust-lang.org/tools/install.

Verify the installation:

```bash
rustc --version
cargo --version
git --version
```

### Clone the Repository

```bash
git clone https://github.com/amirhosseinbahramizadeh/milbus-framework.git
cd milbus-framework
```

Create a working branch:

```bash
git switch -c feature/your-change
```

Choose a descriptive branch name, such as:

```text
feature/protocol-validation
fix/frame-length-check
test/simulator-regression
docs/architecture-guide
```

## 05 / Build and Validation

Before submitting a pull request, run the checks relevant to your change.

### Format

```bash
cargo fmt --all -- --check
```

To apply formatting:

```bash
cargo fmt --all
```

### Compile

```bash
cargo check --workspace
```

### Run Tests

```bash
cargo test --workspace
```

### Run Clippy

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

If the repository's configured toolchain or existing code produces an unrelated warning, document the issue rather than silently suppressing it.

### Test Expectations

Contributors should:

- Add tests for new behavior.
- Add regression tests for confirmed bugs.
- Test malformed and boundary inputs where relevant.
- Verify error handling as well as successful execution.
- Avoid tests that depend on unpredictable timing or external services unless the dependency is explicitly required.
- Ensure tests clean up temporary files and resources.
- Avoid changing expected outputs merely to make failing tests pass without understanding the cause.

A pull request should not be considered verified solely because the project compiles.

## 06 / Rust Engineering Standards

Contributions should follow idiomatic Rust practices and preserve the project's architectural boundaries.

### General Guidelines

- Prefer explicit, descriptive names.
- Use the type system to represent valid states and protocol distinctions.
- Prefer `Result`-based error handling for recoverable failures.
- Avoid unnecessary `unwrap()` and `expect()` calls in production paths.
- Do not introduce `unsafe` code without a compelling reason and a detailed safety explanation.
- Keep functions focused and modules cohesive.
- Avoid unnecessary dependencies.
- Document public APIs when their purpose or constraints are not self-evident.
- Preserve backward compatibility where practical.

### Error Handling

Errors should provide useful context without exposing secrets or leaking sensitive system information.

Do not silently discard errors that could affect protocol correctness, simulation integrity, or test reliability.

### Input Validation

Treat externally supplied frames, capture files, serialized data, configuration values, and command-line inputs as potentially malformed.

Validate lengths, ranges, enumerations, and other relevant invariants before processing data.

### Dependencies

When adding a dependency:

1. Explain why it is necessary.
2. Prefer maintained crates with appropriate licenses.
3. Consider dependency size, transitive dependencies, and security implications.
4. Update `Cargo.toml` and `Cargo.lock` as appropriate.
5. Avoid introducing a dependency for functionality that can be implemented safely and clearly with existing components.

## 07 / Protocol and Simulation Contributions

Protocol-related changes require particular care because similar terminology does not imply identical wire formats or communication behavior.

When implementing or modifying protocol behavior:

- Clearly identify the relevant protocol and feature.
- Distinguish protocol requirements from simulator-specific design choices.
- Validate frame structure and field constraints.
- Document assumptions and unsupported behavior.
- Add representative valid and invalid test cases.
- Include boundary-condition tests where applicable.
- Avoid claiming full standards compliance based on a small number of tests.
- Keep simulated anomalies clearly distinguished from real-world vulnerabilities.

Where feasible, reference authoritative protocol documentation or standards that contributors are legally permitted to access and cite.

Do not include confidential technical data or proprietary material without authorization.

## 08 / Test Design

Tests should be deterministic, maintainable, and focused on observable behavior.

Recommended test categories include:

**Unit tests**
- Frame construction and parsing
- Field validation
- Protocol-specific behavior
- Error conditions and boundary values

**Integration tests**
- Interactions between protocol components
- Virtual bus behavior
- Device-role interactions
- CLI execution and output

**Regression tests**
- Previously confirmed defects
- Unexpected panic conditions
- Incorrect frame handling
- Incorrect state transitions

**Simulation scenario tests**
- Controlled traffic anomalies
- Malformed simulated messages
- Status-handling behavior
- Recovery and error-reporting behavior

Use synthetic or authorized test data. Tests must not target operational avionics or live tactical networks.

## 09 / Documentation Standards

Documentation changes are considered part of the implementation.

When appropriate, update:

- `README.md`
- Relevant crate-level or module-level documentation
- CLI help text and examples
- Test descriptions
- Architecture notes
- The roadmap, when project priorities change

Examples should match the actual CLI and current implementation. Avoid documenting planned functionality as though it were already available.

If a feature has limitations, state them explicitly.

## 10 / Commit Guidelines

Use concise commit messages that describe the change.

Recommended format:

```text
type(scope): short description
```

Examples:

```text
feat(protocol-core): add frame validation
fix(simulator): handle malformed status frames
test(cli): cover invalid simulation arguments
docs(readme): clarify protocol support
refactor(bus): simplify channel routing
ci(rust): enforce workspace lint checks
```

Suggested types:

- `feat` — new functionality
- `fix` — bug fix
- `test` — testing changes
- `docs` — documentation
- `refactor` — code restructuring without intended behavior changes
- `perf` — performance improvements
- `ci` — continuous integration changes
- `build` — build system or dependency changes

Keep commits focused and avoid mixing unrelated changes.

## 11 / Pull Request Procedure

1. Create a branch from the appropriate base branch.
2. Implement the change and add relevant tests.
3. Run formatting, compilation, tests, and lint checks.
4. Update documentation when needed.
5. Commit the changes using descriptive messages.
6. Push the branch to your fork.
7. Open a pull request against the official repository.

### Pull Request Checklist

Before submitting, confirm that:

- [ ] The purpose and scope of the change are clear.
- [ ] The implementation follows existing architectural conventions.
- [ ] Relevant tests have been added or updated.
- [ ] Formatting checks pass.
- [ ] Workspace tests pass, or failures are documented with evidence.
- [ ] Clippy checks pass, or unrelated existing issues are documented.
- [ ] Documentation reflects the actual behavior.
- [ ] No secrets, credentials, private keys, or unrelated generated files are included.
- [ ] Security-sensitive details have not been disclosed publicly.
- [ ] The change does not claim unsupported protocol compliance or operational safety.

### Pull Request Description

A useful pull request should explain:

**Summary**
What changed?

**Motivation**
Why is the change needed?

**Implementation**
Which components were modified, and what design decisions were made?

**Validation**
Which commands and tests were run? What were the results?

**Limitations**
What remains unimplemented, unverified, or dependent on future work?

Include relevant logs or screenshots when they materially help reviewers understand the change.

## 12 / Code Review

Maintainers may request changes, additional tests, clarification, or a narrower scope before merging.

Reviewers should focus on:

- Correctness and reproducibility
- Security and input validation
- Architectural consistency
- Test quality
- Maintainability
- Documentation accuracy
- Unintended behavior changes

Please keep review discussions technical, constructive, and respectful.

Passing CI is necessary when required by repository policy, but it does not by itself establish protocol correctness or safety-critical suitability.

## 13 / Security Reporting

**Do not report undisclosed vulnerabilities through public issues or pull requests.**

Follow `SECURITY.md` for the project's vulnerability reporting process.

Security-related contributions should avoid publishing exploit details that could increase risk before maintainers have had an opportunity to assess the issue.

All testing must be limited to systems and environments for which the contributor has authorization.

## 14 / Scope and Safety

MILBUS Framework is intended for research, development, education, protocol analysis, and controlled simulation.

Contributions must not knowingly introduce functionality intended to facilitate unauthorized access, disruption of operational systems, or unsafe interaction with safety-critical equipment.

The framework is not certified for flight-critical or operational military use. Contributions must not imply certification or guarantee compliance without appropriate evidence.

## 15 / Community Standards

All contributors are expected to communicate professionally and respectfully.

Harassment, discrimination, personal attacks, deliberate disruption of project discussions, and disclosure of another person's private information are not acceptable.

The maintainers may moderate discussions and reject contributions that violate project policies.

## 16 / License and Contribution Terms

By submitting a contribution, you agree that it may be distributed under the license governing the repository, subject to that license's terms.

Before contributing code or documentation:

- Ensure you have the right to submit the material.
- Identify third-party content and applicable license requirements.
- Do not submit proprietary code or confidential data without authorization.
- Preserve relevant copyright and attribution notices.

If the repository does not yet contain a `LICENSE` file, its licensing terms must be clarified before contributors can safely assume that contributions may be redistributed under an open-source license.

## 17 / Maintainer Discretion

Maintainers may request revisions, defer changes, or decline contributions that do not align with project goals, engineering requirements, security expectations, or available maintenance capacity.

Submitting a pull request does not guarantee acceptance or a particular review timeline.

---

<p align="center">
  <strong>MILBUS FRAMEWORK</strong>
  <br/>
  <sub>BUILD WITH PRECISION. TEST WITH DISCIPLINE. CONTRIBUTE RESPONSIBLY.</sub>
</p>
