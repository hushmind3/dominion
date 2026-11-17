-- Money Lab knowledge is indexed independently from chat memory.
CREATE INDEX IF NOT EXISTS idx_money_campaign
    ON records(json_extract(data, '$.campaignId'), updated_at DESC)
    WHERE kind='money_candidate';
CREATE INDEX IF NOT EXISTS idx_money_experiment_candidate
    ON records(json_extract(data,'$.candidateId')) WHERE kind='money_experiment';
CREATE INDEX IF NOT EXISTS idx_money_experiment_campaign
    ON records(json_extract(data, '$.campaignId'), updated_at DESC)
    WHERE kind='money_experiment';
CREATE INDEX IF NOT EXISTS idx_money_metric_entity
    ON records(json_extract(data,'$.entityId'),
       json_extract(data,'$.metric'),json_extract(data,'$.observedAt') DESC)
    WHERE kind='money_metric';
CREATE INDEX IF NOT EXISTS idx_money_metric_change
    ON records(json_extract(data,'$.observedAt') DESC)
    WHERE kind='money_metric_change';
CREATE INDEX IF NOT EXISTS idx_money_event_entity
    ON records(json_extract(data,'$.entityKey'),
               json_extract(data,'$.marketRegion'),
               json_extract(data,'$.effectiveDate'))
    WHERE kind='money_event';
CREATE INDEX IF NOT EXISTS idx_money_event_state
    ON records(json_extract(data,'$.status'), created_at DESC)
    WHERE kind='money_event';
CREATE INDEX IF NOT EXISTS idx_money_asym_state
    ON records(json_extract(data,'$.status'),
               json_extract(data,'$.researchPriority') DESC)
    WHERE kind='money_asymmetry';
CREATE INDEX IF NOT EXISTS idx_money_tension_priority
    ON records(json_extract(data,'$.researchPriority') DESC) WHERE kind='money_tension';
CREATE INDEX IF NOT EXISTS idx_money_relation_from
    ON records(json_extract(data,'$.from')) WHERE kind='money_relation';
CREATE INDEX IF NOT EXISTS idx_money_relation_to
    ON records(json_extract(data,'$.to')) WHERE kind='money_relation';
CREATE INDEX IF NOT EXISTS idx_money_signal_captured
    ON records(json_extract(data,'$.capturedAt')) WHERE kind='money_signal';
CREATE INDEX IF NOT EXISTS idx_money_signal_source_url
    ON records(json_extract(data,'$.url')) WHERE kind='money_signal';
CREATE VIRTUAL TABLE IF NOT EXISTS money_fact_fts
    USING fts5(id UNINDEXED, claim, tokenize='trigram');
CREATE TABLE IF NOT EXISTS money_fact_term(
    term TEXT NOT NULL, fact_id TEXT NOT NULL, PRIMARY KEY(term,fact_id));
CREATE INDEX IF NOT EXISTS idx_money_term_fact ON money_fact_term(fact_id);
