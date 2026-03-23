namespace SEDA.Wpf.Models;

public sealed class ApiEnvelope<T>
{
    public bool Success { get; set; }
    public string? Message { get; set; }
    public T? Data { get; set; }
}

