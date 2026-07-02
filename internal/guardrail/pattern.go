package guardrail

import "regexp"

// patternSpec binds a regex to a coarse metrics bucket.
type patternSpec struct {
	re    *regexp.Regexp
	label string
}

var classifiedPatterns = []patternSpec{
	{regexp.MustCompile(`AKIA[0-9A-Z]{16}`), "aws_key"},
	{regexp.MustCompile(`(?i)(?:password|passwd|pwd)\s*[:=]\s*['"]?[^\s'"]{4,}['"]?`), "other"},
	{regexp.MustCompile(`(?i)(?:api[_-]?key|secret[_-]?key|token)\s*[:=]\s*['"]?[^\s'"]{8,}['"]?`), "api_key"},
	{regexp.MustCompile(`postgres(?:ql)?://[^\s]+`), "connection_string"},
	{regexp.MustCompile(`mysql://[^\s]+`), "connection_string"},
	{regexp.MustCompile(`mongodb(?:\+srv)?://[^\s]+`), "connection_string"},
	{regexp.MustCompile(`redis://[^\s]+`), "connection_string"},
	{regexp.MustCompile(`[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}`), "email"},
	{regexp.MustCompile(`sk-[a-zA-Z0-9]{20,}`), "sk_token"},
	{regexp.MustCompile(`sk-ant-[a-zA-Z0-9\-]{20,}`), "sk_token"},
}

// PatternBucket maps a matched secret to a coarse observability label.
func PatternBucket(match string) string {
	for _, spec := range classifiedPatterns {
		if spec.re.MatchString(match) {
			return spec.label
		}
	}
	return "other"
}

// PatternCounts returns redaction counts grouped by pattern bucket.
func (rm *RedactionMap) PatternCounts() map[string]int {
	if rm == nil {
		return nil
	}
	rm.mu.RLock()
	defer rm.mu.RUnlock()
	if len(rm.patternCounts) == 0 {
		return nil
	}
	out := make(map[string]int, len(rm.patternCounts))
	for k, v := range rm.patternCounts {
		out[k] = v
	}
	return out
}
