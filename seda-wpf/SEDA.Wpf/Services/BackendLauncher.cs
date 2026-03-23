using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Net.Sockets;

namespace SEDA.Wpf.Services;

public sealed class BackendLauncher : IDisposable
{
    private Process? _proc;
    private readonly HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(3) };
    private CancellationTokenSource? _logCts;

    public Uri BaseUri { get; }
    public int Port => BaseUri.Port;
    public string Host => BaseUri.Host;

    public BackendLauncher(Uri baseUri)
    {
        BaseUri = baseUri;
        _http.BaseAddress = baseUri;
    }

    public async Task<bool> EnsureRunningAsync(CancellationToken ct)
    {
        if (await IsHealthyAsync(ct))
            return true;

        StartBackendProcess();

        // Wait up to ~8s for health (backoff)
        var delayMs = 200;
        for (var i = 0; i < 20; i++)
        {
            ct.ThrowIfCancellationRequested();
            if (await IsHealthyAsync(ct))
                return true;
            await Task.Delay(delayMs, ct);
            delayMs = Math.Min(900, (int)(delayMs * 1.25));
        }

        return await IsHealthyAsync(ct);
    }

    private void StartBackendProcess()
    {
        if (_proc is { HasExited: false })
            return;

        // Prefer repo-local venv python in `seda-agent-py/.venv` when running from source.
        // For packaged distribution, set SEDA_PYTHON to a python executable path, or ensure python is on PATH.
        var repoRoot = FindRepoRoot(AppContext.BaseDirectory);
        var backendDir = Path.Combine(repoRoot, "seda-agent-py");
        var venvPython = Path.Combine(backendDir, ".venv", "Scripts", "python.exe");

        var python = Environment.GetEnvironmentVariable("SEDA_PYTHON");
        if (string.IsNullOrWhiteSpace(python))
            python = File.Exists(venvPython) ? venvPython : "python";

        var psi = new ProcessStartInfo
        {
            FileName = python,
            Arguments = "-m seda_agent.main",
            WorkingDirectory = backendDir,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true
        };

        // Make sure the backend binds to the same port as BaseUri.
        psi.Environment["SEDA_MCP_PORT"] = Port.ToString();

        _proc = Process.Start(psi);
        if (_proc is not null)
        {
            StartLogCapture(_proc);
        }
    }

    private async Task<bool> IsHealthyAsync(CancellationToken ct)
    {
        try
        {
            // Prefer an actual HTTP health check over raw port probing.
            using var resp = await _http.GetAsync("/health", ct);
            return resp.IsSuccessStatusCode;
        }
        catch
        {
            // Fall back to port probe (useful during very early bind).
            return await PortOpenAsync(Host, Port, ct);
        }
    }

    private static async Task<bool> PortOpenAsync(string host, int port, CancellationToken ct)
    {
        try
        {
            using var client = new TcpClient();
            var connectTask = client.ConnectAsync(host, port);
            var completed = await Task.WhenAny(connectTask, Task.Delay(450, ct));
            return completed == connectTask && client.Connected;
        }
        catch
        {
            return false;
        }
    }

    private void StartLogCapture(Process proc)
    {
        try
        {
            _logCts?.Cancel();
            _logCts?.Dispose();
            _logCts = new CancellationTokenSource();

            var logPath = GetLogPath();
            Directory.CreateDirectory(Path.GetDirectoryName(logPath)!);

            _ = Task.Run(async () =>
            {
                await using var fs = new FileStream(logPath, FileMode.Append, FileAccess.Write, FileShare.ReadWrite);
                await using var sw = new StreamWriter(fs) { AutoFlush = true };

                await sw.WriteLineAsync($"[{DateTime.Now:O}] Backend started (pid={proc.Id})");

                async Task pump(StreamReader reader, string tag)
                {
                    while (!_logCts!.IsCancellationRequested)
                    {
                        var line = await reader.ReadLineAsync();
                        if (line is null) break;
                        await sw.WriteLineAsync($"[{DateTime.Now:O}] {tag} {line}");
                    }
                }

                await Task.WhenAll(
                    pump(proc.StandardOutput, "OUT"),
                    pump(proc.StandardError, "ERR")
                );
            }, _logCts.Token);
        }
        catch
        {
            // best-effort
        }
    }

    private static string GetLogPath()
    {
        var baseDir = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        return Path.Combine(baseDir, "SEDA", "logs", "backend.log");
    }

    private static string FindRepoRoot(string startDir)
    {
        var dir = new DirectoryInfo(startDir);
        while (dir.Parent is not null)
        {
            var candidate = Path.Combine(dir.FullName, "seda-agent-py");
            if (Directory.Exists(candidate))
                return dir.FullName;
            dir = dir.Parent;
        }
        return startDir;
    }

    public void Dispose()
    {
        try
        {
            _logCts?.Cancel();
            if (_proc is { HasExited: false })
            {
                _proc.Kill(entireProcessTree: true);
                _proc.Dispose();
            }
            _http.Dispose();
        }
        catch
        {
            // best-effort
        }
    }
}

