//! Allowed JSON Schema 2020-12 keyword set for Kotro's security profile.

/// Dialects Kotro admits. Missing `$schema` is treated as 2020-12.
pub const DRAFT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Formats that Kotro asserts (not merely annotates).
pub const ASSERTED_FORMATS: &[&str] = &[
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "uuid",
];

/// Standard keywords Kotro's security profile permits.
pub const ALLOWED_KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "$anchor",
    "$ref",
    "$defs",
    "$comment",
    "title",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "type",
    "enum",
    "const",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentRequired",
    "dependentSchemas",
    "properties",
    "patternProperties",
    "propertyNames",
    "additionalProperties",
    "unevaluatedProperties",
    "required",
    "minProperties",
    "maxProperties",
    "prefixItems",
    "items",
    "contains",
    "minContains",
    "maxContains",
    "unevaluatedItems",
    "minItems",
    "maxItems",
    "uniqueItems",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
    "contentEncoding",
    "contentMediaType",
];

/// Keywords that force quarantine until fully supported under Kotro's profile.
pub const QUARANTINE_KEYWORDS: &[&str] = &[
    "$dynamicRef",
    "$dynamicAnchor",
    "$vocabulary",
    "$recursiveRef",
    "$recursiveAnchor",
    "contentSchema",
];

pub fn is_allowed_keyword(key: &str) -> bool {
    key.starts_with("x-") || ALLOWED_KEYWORDS.contains(&key)
}

pub fn is_quarantine_keyword(key: &str) -> bool {
    QUARANTINE_KEYWORDS.contains(&key)
}

pub fn is_asserted_format(fmt: &str) -> bool {
    ASSERTED_FORMATS.contains(&fmt)
}

pub fn is_draft_2020_12(uri: &str) -> bool {
    uri == DRAFT_2020_12
        || uri == "http://json-schema.org/draft/2020-12/schema"
        || uri.ends_with("/draft/2020-12/schema")
}
