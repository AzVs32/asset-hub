CREATE TABLE resource_kind_metadata_layers (
    resource_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (resource_id, kind),
    FOREIGN KEY (resource_id) REFERENCES resources(id) ON DELETE CASCADE,
    CHECK (schema_version > 0),
    CHECK (json_valid(payload_json)),
    CHECK (json_type(payload_json) = 'object')
);

INSERT INTO resource_kind_metadata_layers (
    resource_id,
    kind,
    schema_version,
    payload_json
)
SELECT
    resource_id,
    kind,
    schema_version,
    payload_json
FROM resource_kind_metadata;

DROP TABLE resource_kind_metadata;

ALTER TABLE resource_kind_metadata_layers
RENAME TO resource_kind_metadata;

CREATE INDEX idx_resource_kind_metadata_kind
ON resource_kind_metadata(kind);

