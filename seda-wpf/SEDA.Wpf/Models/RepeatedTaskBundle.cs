using System.Text.Json;
using System.Text.Json.Serialization;

namespace SEDA.Wpf.Models;

public sealed class RepeatedTaskBundle
{
    [JsonPropertyName("pattern_hash")]
    public string? PatternHash { get; set; }

    [JsonPropertyName("sequence")]
    public List<string>? Sequence { get; set; }

    [JsonPropertyName("sequence_label")]
    public string? SequenceLabel { get; set; }

    [JsonPropertyName("frequency")]
    public int Frequency { get; set; }

    [JsonPropertyName("avg_duration_ms")]
    public int? AvgDurationMs { get; set; }

    [JsonPropertyName("last_seen_iso")]
    public string? LastSeenIso { get; set; }

    [JsonPropertyName("sample_run")]
    public List<Dictionary<string, JsonElement>>? SampleRunRaw { get; set; }
}

