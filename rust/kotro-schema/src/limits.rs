//! Resource limits for schema admission and argument validation.

/// Configurable limits with hard caps from the S3 / C5 contract.
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub encoded_schema_size: usize,
    pub schema_nesting_depth: u32,
    pub schema_nodes: u32,
    pub local_references: u32,
    pub reference_resolution_depth: u32,
    pub combinator_branches: u32,
    pub regex_count: u32,
    pub regex_length: usize,
    pub enum_entries: u32,
    pub enum_serialized_bytes: usize,
    pub schema_compile_ms: u64,
    pub encoded_arguments_size: usize,
    pub argument_nesting_depth: u32,
    pub argument_nodes: u32,
    pub individual_string: usize,
    pub array_elements: u32,
    pub object_properties: u32,
    pub validation_errors_retained: u32,
    pub validation_deadline_ms: u64,
}

impl ResourceLimits {
    pub const HARD: Self = Self {
        encoded_schema_size: 256 * 1024,
        schema_nesting_depth: 32,
        schema_nodes: 4_096,
        local_references: 512,
        reference_resolution_depth: 64,
        combinator_branches: 256,
        regex_count: 256,
        regex_length: 1_024,
        enum_entries: 1_024,
        enum_serialized_bytes: 256 * 1024,
        schema_compile_ms: 50,
        encoded_arguments_size: 1024 * 1024,
        argument_nesting_depth: 64,
        argument_nodes: 100_000,
        individual_string: 256 * 1024,
        array_elements: 10_000,
        object_properties: 10_000,
        validation_errors_retained: 32,
        validation_deadline_ms: 100,
    };

    pub fn initial() -> Self {
        Self {
            validation_deadline_ms: 25,
            ..Self::HARD
        }
    }

    /// Clamp every field to its hard maximum.
    pub fn clamp(mut self) -> Self {
        let h = Self::HARD;
        self.encoded_schema_size = self.encoded_schema_size.min(h.encoded_schema_size);
        self.schema_nesting_depth = self.schema_nesting_depth.min(h.schema_nesting_depth);
        self.schema_nodes = self.schema_nodes.min(h.schema_nodes);
        self.local_references = self.local_references.min(h.local_references);
        self.reference_resolution_depth = self
            .reference_resolution_depth
            .min(h.reference_resolution_depth);
        self.combinator_branches = self.combinator_branches.min(h.combinator_branches);
        self.regex_count = self.regex_count.min(h.regex_count);
        self.regex_length = self.regex_length.min(h.regex_length);
        self.enum_entries = self.enum_entries.min(h.enum_entries);
        self.enum_serialized_bytes = self.enum_serialized_bytes.min(h.enum_serialized_bytes);
        self.schema_compile_ms = self.schema_compile_ms.min(h.schema_compile_ms);
        self.encoded_arguments_size = self.encoded_arguments_size.min(h.encoded_arguments_size);
        self.argument_nesting_depth = self.argument_nesting_depth.min(h.argument_nesting_depth);
        self.argument_nodes = self.argument_nodes.min(h.argument_nodes);
        self.individual_string = self.individual_string.min(h.individual_string);
        self.array_elements = self.array_elements.min(h.array_elements);
        self.object_properties = self.object_properties.min(h.object_properties);
        self.validation_errors_retained = self
            .validation_errors_retained
            .min(h.validation_errors_retained);
        self.validation_deadline_ms = self.validation_deadline_ms.min(h.validation_deadline_ms);
        self
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::initial()
    }
}
