using System.Text.Json.Serialization;

namespace SEDA.Wpf.Models;

public sealed class DashboardAction
{
    [JsonPropertyName("id")]
    public int Id { get; set; }

    [JsonPropertyName("action_type")]
    public string? ActionType { get; set; }

    [JsonPropertyName("source_app")]
    public string? SourceApp { get; set; }

    [JsonPropertyName("target_app")]
    public string? TargetApp { get; set; }

    [JsonPropertyName("website_domain")]
    public string? WebsiteDomain { get; set; }

    [JsonPropertyName("search_query")]
    public string? SearchQuery { get; set; }

    [JsonPropertyName("session_id")]
    public string? SessionId { get; set; }

    [JsonPropertyName("timestamp_iso")]
    public string? TimestampIso { get; set; }
}

