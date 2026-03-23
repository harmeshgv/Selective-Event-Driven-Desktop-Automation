using System.Net.Http;

namespace SEDA.Wpf.Services;

public sealed class AppServices : IDisposable
{
    public Uri BaseUri { get; }
    public BackendLauncher Launcher { get; }
    public SedaApiClient Api { get; }

    public AppServices(Uri baseUri)
    {
        var envUrl = Environment.GetEnvironmentVariable("SEDA_BACKEND_URL");
        BaseUri = Uri.TryCreate(envUrl, UriKind.Absolute, out var parsed) ? parsed : baseUri;
        Launcher = new BackendLauncher(BaseUri);
        Api = new SedaApiClient(new HttpClient
        {
            BaseAddress = BaseUri,
            Timeout = TimeSpan.FromSeconds(10)
        });
    }

    public void Dispose() => Launcher.Dispose();
}

