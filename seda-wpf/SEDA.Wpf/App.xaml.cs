using System.Windows;
using SEDA.Wpf.Services;

namespace SEDA.Wpf;

public partial class App : Application
{
    public static AppServices Services { get; private set; } = null!;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        Services = new AppServices(new Uri("http://127.0.0.1:9315"));
    }

    protected override void OnExit(ExitEventArgs e)
    {
        try
        {
            Services.Dispose();
        }
        catch
        {
            // best-effort
        }
        base.OnExit(e);
    }
}

