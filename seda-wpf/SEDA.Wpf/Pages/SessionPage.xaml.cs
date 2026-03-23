using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows;
using System.Windows.Controls;
using SEDA.Wpf.Services;

namespace SEDA.Wpf.Pages;

public partial class SessionPage : Page, INotifyPropertyChanged
{
    private readonly AppServices _services;
    private readonly MainWindow _shell;

    private string _statusLine = "Connecting…";
    public string StatusLine { get => _statusLine; set => Set(ref _statusLine, value); }

    private string _detailsLine = "";
    public string DetailsLine { get => _detailsLine; set => Set(ref _detailsLine, value); }

    private string _errorLine = "";
    public string ErrorLine { get => _errorLine; set => Set(ref _errorLine, value); }

    public SessionPage(AppServices services, MainWindow shell)
    {
        InitializeComponent();
        DataContext = this;
        _services = services;
        _shell = shell;
        Loaded += async (_, _) => await RefreshStatusAsync();
    }

    private async Task RefreshStatusAsync()
    {
        ErrorLine = "";
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(6));
            var env = await _services.Api.GetStatusAsync(cts.Token);
            var s = env?.Data;
            var collecting = s?.Collecting == true;
            var sid = s?.SessionId ?? "";

            _shell.StatusText = collecting ? $"Collecting ({sid[..Math.Min(8, sid.Length)]})" : "Idle";
            StatusLine = collecting ? "Collecting" : "Idle";
            DetailsLine = $"Session: {(string.IsNullOrWhiteSpace(sid) ? "—" : sid)}";
        }
        catch (Exception ex)
        {
            _shell.StatusText = "Error";
            StatusLine = "Backend error";
            ErrorLine = ex.Message;
        }
    }

    private async void Start_OnClick(object sender, RoutedEventArgs e)
    {
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(6));
            await _services.Api.StartAsync(cts.Token);
            await RefreshStatusAsync();
        }
        catch (Exception ex)
        {
            ErrorLine = ex.Message;
        }
    }

    private async void Stop_OnClick(object sender, RoutedEventArgs e)
    {
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(6));
            await _services.Api.StopAsync(cts.Token);
            await RefreshStatusAsync();
        }
        catch (Exception ex)
        {
            ErrorLine = ex.Message;
        }
    }

    private async void Clear_OnClick(object sender, RoutedEventArgs e)
    {
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(6));
            await _services.Api.ClearAsync(cts.Token);
            await RefreshStatusAsync();
        }
        catch (Exception ex)
        {
            ErrorLine = ex.Message;
        }
    }

    private async void Refresh_OnClick(object sender, RoutedEventArgs e) => await RefreshStatusAsync();

    public event PropertyChangedEventHandler? PropertyChanged;
    private void OnPropertyChanged([CallerMemberName] string? name = null)
        => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));
    private void Set<T>(ref T field, T value, [CallerMemberName] string? name = null)
    {
        if (Equals(field, value)) return;
        field = value;
        OnPropertyChanged(name);
    }
}

