using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using SEDA.Wpf.Models;

namespace SEDA.Wpf.Services;

public sealed class SedaApiClient
{
    private readonly HttpClient _http;
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNameCaseInsensitive = true
    };

    public SedaApiClient(HttpClient http)
    {
        _http = http;
    }

    public async Task<bool> HealthAsync(CancellationToken ct)
    {
        using var resp = await _http.GetAsync("/health", ct);
        return resp.IsSuccessStatusCode;
    }

    public async Task<ApiEnvelope<CollectorSnapshot>?> GetStatusAsync(CancellationToken ct)
        => await GetJsonAsync<ApiEnvelope<CollectorSnapshot>>("/api/dashboard/status", ct);

    public async Task<ApiEnvelope<CollectorSnapshot>?> StartAsync(CancellationToken ct)
        => await PostAsync<ApiEnvelope<CollectorSnapshot>>("/api/dashboard/start", ct);

    public async Task<ApiEnvelope<CollectorSnapshot>?> StopAsync(CancellationToken ct)
        => await PostAsync<ApiEnvelope<CollectorSnapshot>>("/api/dashboard/stop", ct);

    public async Task<ApiEnvelope<CollectorSnapshot>?> ClearAsync(CancellationToken ct)
        => await PostAsync<ApiEnvelope<CollectorSnapshot>>("/api/dashboard/clear", ct);

    public async Task<ApiEnvelope<List<RepeatedTaskBundle>>?> GetRepeatedTasksAsync(
        int minFrequency,
        int limit,
        int flowLimit,
        CancellationToken ct)
    {
        var url = $"/api/dashboard/repeated_tasks?min_frequency={minFrequency}&limit={limit}&flow_limit={flowLimit}";
        return await GetJsonAsync<ApiEnvelope<List<RepeatedTaskBundle>>>(url, ct);
    }

    private async Task<T?> GetJsonAsync<T>(string path, CancellationToken ct)
    {
        try
        {
            return await _http.GetFromJsonAsync<T>(path, JsonOptions, ct);
        }
        catch (TaskCanceledException ex)
        {
            throw new HttpRequestException($"Request timed out: {path}", ex);
        }
        catch (Exception ex)
        {
            throw new HttpRequestException($"Request failed: {path}", ex);
        }
    }

    private async Task<T?> PostAsync<T>(string path, CancellationToken ct)
    {
        try
        {
            using var resp = await _http.PostAsync(path, content: null, ct);
            resp.EnsureSuccessStatusCode();
            return await resp.Content.ReadFromJsonAsync<T>(JsonOptions, ct);
        }
        catch (TaskCanceledException ex)
        {
            throw new HttpRequestException($"Request timed out: {path}", ex);
        }
        catch (Exception ex)
        {
            throw new HttpRequestException($"Request failed: {path}", ex);
        }
    }
}

