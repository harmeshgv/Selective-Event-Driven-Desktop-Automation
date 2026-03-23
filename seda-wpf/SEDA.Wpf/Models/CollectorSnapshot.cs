namespace SEDA.Wpf.Models;

public sealed class CollectorSnapshot
{
    public bool Collecting { get; set; }
    public string? SessionId { get; set; }
    public long? StartedMs { get; set; }
    public int? ActionCount { get; set; }
}

