# Security Policy — MILBUS Framework

<p align="center">
  <strong>MILBUS FRAMEWORK | SECURITY & RESPONSIBLE DISCLOSURE</strong>
</p>

---

## 1. Overview

MILBUS Framework is an open-source Rust framework for avionics communication protocol simulation, protocol analysis, virtual bus environments, and controlled fault-injection testing.

Security is an important consideration in the design, implementation, testing, and maintenance of the project.

This document defines the project's security support policy, vulnerability reporting process, and expectations for responsible disclosure.

## 2. Supported Versions

Security fixes are prioritized for the latest maintained version.

| Version | Security support |
|---|---|
| Latest stable release | Supported, subject to maintainer availability |
| Development branch | Best-effort review and remediation |
| Older releases | Not guaranteed |

If the project has not published a stable release, security support should be understood as best-effort until a formal release policy is established.

Users should upgrade to the latest supported release when security fixes become available.

## 3. Reporting a Vulnerability

**Do not publicly disclose unpatched vulnerabilities through GitHub Issues, public pull requests, or public discussions.**

If you believe you have identified a security vulnerability in MILBUS Framework, report it privately using one of the following methods:

1. **GitHub Private Vulnerability Reporting:** Use the repository's *Security → Advisories → Report a vulnerability* workflow, if enabled.
2. **Private maintainer contact:** Contact the repository maintainers through an officially published security contact, if one is available.

Do not assume that a security contact exists until the maintainers have published and verified it.

If neither method is available, contact the maintainers privately before publishing technical details.

### Suggested Report Contents

Include as much of the following information as possible:

- **Summary:** A concise description of the suspected vulnerability.
- **Affected component:** The relevant crate, module, command, or protocol implementation.
- **Affected versions:** Releases, branches, or commits believed to be affected.
- **Severity and impact:** The potential consequences and realistic attack prerequisites.
- **Reproduction steps:** Minimal steps required to reproduce the behavior.
- **Proof of concept:** A minimal, non-destructive example, where appropriate.
- **Expected behavior:** What should happen under the same conditions.
- **Observed behavior:** What actually happens, including relevant logs or error messages.
- **Suggested remediation:** Any proposed mitigation or fix, if known.

Please remove secrets, credentials, personal data, and unrelated sensitive information from reports and attached logs.

## 4. Scope

The following areas may be relevant to security reports, depending on the behavior demonstrated:

- Memory safety and panic conditions in Rust code.
- Incorrect validation or parsing of protocol frames.
- Integer overflow, length-validation errors, and malformed-input handling.
- Denial-of-service conditions caused by untrusted or malformed simulation inputs.
- Unsafe code, dependency vulnerabilities, and supply-chain risks.
- Incorrect isolation between virtual buses, channels, or simulated device roles.
- Unauthorized access to files or resources, where applicable to supported functionality.
- Insecure handling of capture files, configuration, logs, or serialized data.
- CLI or TUI behavior that creates a demonstrable security impact.
- Security defects in simulation and fault-injection mechanisms.

A finding should include a reproducible security impact or a technically supported explanation of the risk.

## 5. Out of Scope

Unless a concrete security impact is demonstrated, the following are generally not considered security vulnerabilities:

- Issues limited to unsupported or undocumented functionality.
- Test-only failures without a security consequence.
- Cosmetic defects, formatting issues, and ordinary usability problems.
- Theoretical vulnerabilities without a credible attack path.
- Performance differences without a meaningful availability or security impact.
- Findings that depend entirely on incorrect configuration or unsupported deployment assumptions.
- Simulated protocol anomalies that do not demonstrate a defect in the framework itself.

Reports involving third-party software should identify the affected component and distinguish upstream issues from vulnerabilities introduced by MILBUS Framework.

## 6. Responsible Disclosure

The project asks security researchers to:

- Make a good-faith effort to avoid disrupting systems or services.
- Use local, isolated, or otherwise authorized test environments.
- Avoid testing against operational avionics, live tactical networks, or safety-critical infrastructure.
- Avoid accessing, modifying, or exfiltrating data that does not belong to them.
- Avoid destructive testing and unnecessary denial-of-service activity.
- Keep vulnerability details confidential while a report is being assessed and remediated.
- Coordinate public disclosure with the maintainers.

Do not use MILBUS Framework or a reported vulnerability to access systems without explicit authorization.

The maintainers may coordinate disclosure timing with the reporter based on severity, remediation complexity, and the availability of a fix.

## 7. Response and Remediation

After receiving a report, the maintainers will make a reasonable effort to:

1. Acknowledge receipt.
2. Review the report and attempt to reproduce the issue.
3. Assess severity, affected components, and potential impact.
4. Determine an appropriate remediation or mitigation.
5. Coordinate disclosure with the reporter when feasible.
6. Publish a security advisory or release note when appropriate.

Response and remediation timelines depend on maintainer availability, report quality, severity, and technical complexity. No fixed response-time guarantee is implied by this policy.

If a report is not reproducible or does not demonstrate a security impact, the maintainers may request additional evidence or close the report with an explanation.

## 8. Severity Assessment

The maintainers may use the Common Vulnerability Scoring System (CVSS) as a reference when assessing security issues.

Severity will be evaluated in context, considering:

- Attack prerequisites and required privileges.
- Whether the affected component processes untrusted input.
- Confidentiality, integrity, and availability impact.
- Whether the issue affects only a local simulation or can cross a meaningful security boundary.
- Reproducibility and practical exploitability.
- The distinction between simulated behavior and real-world system impact.

A protocol simulation result alone does not establish a vulnerability in deployed avionics equipment.

## 9. Security Advisories

When appropriate, confirmed vulnerabilities may be documented through GitHub Security Advisories or release notes.

An advisory may include:

- Affected versions and fixed versions.
- A description of the issue and its impact.
- Mitigation and upgrade instructions.
- Relevant acknowledgments, where agreed with the reporter.

Sensitive reproduction details may be withheld or published in stages when immediate disclosure could increase risk.

## 10. Dependencies and Supply Chain

Contributors and maintainers are encouraged to:

- Keep dependencies reasonably up to date.
- Review security advisories affecting Rust crates and build tooling.
- Minimize unnecessary dependencies.
- Review changes to dependency versions and lockfiles.
- Use automated dependency and vulnerability checks where practical.
- Avoid committing credentials, private keys, tokens, or other secrets.
- Treat externally supplied capture files, protocol frames, and configuration files as potentially untrusted input.

Automated scanners are useful signals, but their findings should be evaluated for actual applicability and impact.

## 11. Safety-Critical Use Disclaimer

MILBUS Framework is provided for research, development, education, protocol analysis, and controlled simulation.

It is **not certified or warranted for flight-critical, operational military, or other safety-critical use**. It must not be treated as a validated replacement for certified avionics equipment, engineering verification, or formal safety-assurance processes.

Users are responsible for evaluating the risks of their deployment and for obtaining the necessary authorization before testing systems.

## 12. Acknowledgments

The project may acknowledge researchers who report valid vulnerabilities responsibly, subject to their consent.

Acknowledgment does not imply a bounty, payment, employment, endorsement, or any other compensation unless the maintainers explicitly establish such a program.

## 13. Policy Updates

This policy may be updated as MILBUS Framework matures, releases are published, and the project's security reporting capabilities evolve.

For the latest policy, refer to the `SECURITY.md` file in the official repository.

---

**MILBUS Framework — Security through verification, responsible research, and controlled testing.**
