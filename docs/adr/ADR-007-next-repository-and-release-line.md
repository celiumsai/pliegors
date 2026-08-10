# ADR-007: PliegoRS Next evolves the existing repository on the 0.5 line

**Status:** Accepted
**Decision date:** 2026-08-08
**Scope:** repository topology, stewardship, branches, package identity, and release numbering

## Context

The initial PliegoRS Next proposal assumed a greenfield workspace, a persistent
`next` branch, release numbering beginning at `0.1`, and several package names
that already exist as published `0.4.0-beta.1` contracts.

The repository currently publishes twenty-one coordinated Rust crates, signed
release artifacts, a canonical capability manifest, and accepted event,
projection, lifecycle, artifact, server, and deployment evidence. Its governance
and release automation use `main` as the sole persistent and release-producing
branch. Celiums Solutions LLC is the current steward, licensor, trademark owner,
and registry authority.

Resetting package versions or redefining current package names would make a new
chronological release appear older than existing packages and would break the
meaning of released contracts without migration.

## Decision

1. PliegoRS Next is a development codename for the coordinated `0.5.x` line of
   the existing PliegoRS product.
2. Work remains in `celiumsai/pliegors` under Celiums Solutions LLC.
3. `main` remains the sole persistent branch. Feature and experiment branches
   are short-lived and merge through the existing review and verification path.
4. The released legacy authority is tag `v0.4.0-beta.1` at
   `9cdadea508dfbf78b2ec5061df6846e7fa727211`.
5. The technical pre-Next branch point is
   `bdc285fbe208f09c47ffdfbf1081e923884a9cf7`. It is an inventory reference, not
   a replacement for released evidence.
6. The first public Next release uses valid coordinated SemVer on the `0.5`
   line, normally `0.5.0-beta.1`. A public alpha requires an explicit release-
   channel decision and would use `0.5.0-alpha.1`.
7. Existing published package names retain their current responsibility. In
   particular, `pliego-runtime` remains the native HTTP runtime.
8. G4 through G7 are integrated into the `0.5` program. Existing completion
   evidence remains a regression gate and is not reopened without contradictory
   evidence.
9. The canonical capability manifest continues to distinguish released,
   source-preview, partial, and unavailable surfaces. New crates are not
   published or counted as released until their gates close.

## Consequences

- The project avoids a parallel history, duplicate issue tracker, and split
  release authority.
- Existing users receive normal pre-1.0 migration guidance instead of a package
  identity reset.
- A long-lived legacy branch is unnecessary unless a concrete backport promise
  is adopted later.
- The repository structure evolves only where a boundary needs a distinct
  package; it is not rearranged to resemble a greenfield diagram.
- Any public API, schema, security, platform, or governance implementation still
  follows the public-issue requirement in `GOVERNANCE.md`.

## Rejected alternatives

- **Separate repository with the same package names:** rejected because registry
  and product authority would be ambiguous.
- **Persistent `next` and `legacy` branches:** rejected because they conflict
  with current release automation and add backport obligations without a support
  commitment.
- **Restart at `0.1.0`:** rejected because the existing coordinated packages are
  already at `0.4.0-beta.1`.
- **Rename the steward in documentation only:** rejected because copyright,
  trademarks, GitHub, registry ownership, and release authority require a legal
  and operational transition.

## References

- [PliegoRS 0.5 roadmap](../../ROADMAP.md)
- [Transition program](../52-pliegors-next-transition.md)
- [Product constitution](../34-product-constitution.md)
- [Project governance](../../GOVERNANCE.md)
- [Distribution and release](../27-distribution-and-release.md)
