# Sub2 Money — original-source lineage, syndication-safe cross-market pairing (2026-11-18)

Branch: `dominion-sub2`. Scope: existing `money_ingest` → SQLite source archive → `money_fold` → deterministic events → market asymmetry and economic-tension candidates. The existing Money API, UI, scheduler, user review and implementation approvals remain unchanged.

## Actual defects

1. `sources::dedupe_titles` dropped a **different URL** from another feed if its normalized headline matched recent history. Thus a subsequent original article, correction, translation or cross-region observation could disappear from the saved archive before a human/LLM could inspect it.
2. `sources::run` treated an all-duplicate/empty-but-successful feed scan as a failed job; as a result, a normal no-change cycle could be misclassified as an operational error.
3. The scheduled ingest passed only `history` (previously archived signals) to `money_mechanism::discover`; a buyer request detected in this cycle could not join an earlier supplier cost drop until the next scheduled ingest.
4. Two Google News RSS search channels can point to the *same* underlying publication. Treating feed labels or distinct `news.google.com` article URLs as independent publishers creates counterfeit two-source corroboration.
5. Existing source and event archive rows lacked an incremental path for a newly obtained publisher identity or additional RSS search feed provenance.

## Invariants and implementation

```text
RSS feeds (zero LLM tokens)
 → parse original item URL, headline, publisher <source url>, source channel
 → exact URL dedup within the batch, merge up to 12 observed feed paths
 → stable normalized headline SHA-256 storyClusterId
 → repeated-title label (not verified independent publication)
 → SQLite transaction: first capture and original content preserved;
      backfill missing publisher metadata, extra feed paths or fetched body
 → fold with unchanged event ID and prior paired/evaluated status
 → same-cycle 120-signal bounded research window (fresh observations first)
 → publisher-domain and story-cluster admission gates
 → pending research hypotheses, not verified paid orders
```

- `src/engine/money/sources.rs`: `publisherDomain` comes from the original RSS item `<source url>` when present. For direct article URLs, URL host is used. An RSS URL under `news.google.com` is explicitly an *aggregator*, not a publisher. The original feed name is retained separately as `sourceName`, and the different observation paths are stored in `observedFeeds`.
- **One URL, one article record.** A repeated URL within the batch contributes an additional feed origin (bounded); a new URL with an identical title remains a distinct archived source observation. Normalized-title SHA-256 groups potentially syndicated stories. All such matches have `independentConfirmation=false`; neither an arbitrary number of articles nor two search channels establishes buyer intent, sales or independent evidence.
- `src/db/money.rs`: transactional update of only missing publisher/cluster attribution and bounded RSS origins. Old `title`, `description`, `capturedAt`, existing excerpt and decision status are preserved. A newly enriched source is marked for the normal, existing fold job; an existing event is never silently replaced or re-paired when its origin is backfilled.
- `src/engine/money/folding.rs`: event rows carry original `publisherName`, `publisherDomain`, `storyClusterId`, `storyRelationship`.
- `src/engine/money/mechanism.rs`: a verified publisher identifier is required for a two-source market tension. Publisher-domain normalization collapses subdomains; unmatched Google News hosts and same-story clusters are excluded.
- `src/engine/money/asymmetry.rs`: regional, temporal, channel and interpretive hypotheses require two different recognized publisher domains and must not be exact same-title story clusters. This limits false research suggestions without promoting a hypothesis to a verified transaction.
- Search/matching stays model-free, bounded to a 120-signal current window. The new signal is considered during the ingest *that collected it*. No second agent run loop, model prompt, RAG collection, DB schema migration, background process or timer has been added.
- Normal successful scans with zero valid articles record an empty ingest summary, while scans with no successfully fetched feeds report an actual error. Fetching the same article URL more than once in a single cycle is avoided.

## Concrete regression coverage

- distinct original article URLs with identical headlines persist; repeated exact URLs merge their feed origins; matched titles do not claim independent confirmation;
- Google News RSS `<source url>` publisher attribution and missing-publisher fallback;
- newly collected observations take precedence over stale rows during candidate pairing;
- old source rows retain title/content/capture time when newer publisher, article body and feed observations appear; re-ingesting is idempotent;
- existing event ID, quote and paired status remain unchanged when publisher attribution is backfilled;
- same-story and shared-publisher sources do not create cross-region asymmetry candidates; direct original publishers can still create pending, explicitly unverified hypotheses;
- news.google.com without source metadata never counts as an independent publisher.

## Research tradeoff

Relevant 2026 work distinguishes repeated articles from independent observations:
- [Copies Are Not Confirmations (2026-11-06)](https://arxiv.org/abs/2611.09203) highlights the danger of counting syndication copies as evidence and the remaining difficulty of rewritten or colluding sources.
- [Copies or Sources? (2026-10-05)](https://arxiv.org/abs/2610.06192) analyzes repeated statements and spurious multiagent source weights.
- [SEER, knowledge graphs from news with extraction and attribution (2026-10)](https://doi.org/10.1016/j.knosys.2026.116890) motivates separating extraction from evidence attribution.

This sub2 patch implements **exact headline clustering plus publisher attribution**, not the papers' full paraphrase/entity-graph detection or multiagent learning algorithms. Identical normalized headlines across two publications stay separately archived but are not promoted as independent confirmations; different headlines may still paraphrase the same upstream source. Real market-research quality gain, token spend and wall time remain to be measured on identical inputs.

## Run (Rust dev tests only)

```sh
cargo test --lib money_sources
cargo test --lib money_folding
cargo test --lib money_mechanism
cargo test --lib money_asymmetry
cargo test --lib syndicated_feeds_enrich
cargo test --lib folded_event_attribution_backfill
```

No user accounts, OAuth credentials, legacy SQLite records, main/sub3/sub4 branches, SDK endpoints, or React screens are modified.
