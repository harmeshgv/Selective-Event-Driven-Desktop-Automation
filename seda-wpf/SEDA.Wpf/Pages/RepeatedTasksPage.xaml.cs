using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Windows;
using System.Windows.Controls;
using SEDA.Wpf.Models;
using SEDA.Wpf.Services;

namespace SEDA.Wpf.Pages;

public partial class RepeatedTasksPage : Page, INotifyPropertyChanged
{
    private readonly AppServices _services;
    private readonly MainWindow _shell;

    public ObservableCollection<BundleVm> Bundles { get; } = new();
    private List<RepeatedTaskBundle> _rawBundles = new();

    private string _minRepeats = "2";
    public string MinRepeats { get => _minRepeats; set => Set(ref _minRepeats, value); }

    private string _limit = "25";
    public string Limit { get => _limit; set => Set(ref _limit, value); }

    private string _query = "";
    public string Query { get => _query; set => Set(ref _query, value); }

    private string _previewLine = "Load bundles to preview details.";
    public string PreviewLine { get => _previewLine; set => Set(ref _previewLine, value); }

    private string _errorLine = "";
    public string ErrorLine { get => _errorLine; set => Set(ref _errorLine, value); }

    public RepeatedTasksPage(AppServices services, MainWindow shell)
    {
        InitializeComponent();
        DataContext = this;
        _services = services;
        _shell = shell;
        Loaded += async (_, _) => await LoadAsync();
    }

    private async Task LoadAsync()
    {
        ErrorLine = "";
        PreviewLine = "Loading…";
        Bundles.Clear();
        _rawBundles = new();

        try
        {
            var minRepeats = ParseIntOrDefault(MinRepeats, 2);
            var limit = ParseIntOrDefault(Limit, 25);
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
            var env = await _services.Api.GetRepeatedTasksAsync(minRepeats, limit, 5000, cts.Token);
            var bundles = env?.Data ?? new List<RepeatedTaskBundle>();

            // status line
            _shell.StatusText = "Idle";

            _rawBundles = bundles;
            var q = (Query ?? "").Trim().ToLowerInvariant();
            var filtered = string.IsNullOrEmpty(q)
                ? bundles
                : bundles
                    .Where(b => (b.SequenceLabel ?? "").ToLowerInvariant().Contains(q))
                    .ToList();

            if (filtered.Count == 0)
            {
                Bundles.Add(new BundleVm("No repeated tasks yet", "Start a session and repeat a workflow 2+ times.") { IsEnabled = false });
                PreviewLine = "No bundles to show.";
                return;
            }

            foreach (var b in filtered)
            {
                var steps = b.Sequence?.Count ?? 0;
                var title = $"{steps} steps · x{b.Frequency}";
                var subtitle = b.SequenceLabel ?? "";
                Bundles.Add(new BundleVm(title, subtitle));
            }

            PreviewLine = "Select a bundle to preview, or open Bundle Details page to inspect actions.";
        }
        catch (Exception ex)
        {
            PreviewLine = "Failed to load bundles.";
            ErrorLine = ex.Message;
        }
    }

    private async void Load_OnClick(object sender, RoutedEventArgs e) => await LoadAsync();

    private void Bundles_OnSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        var list = sender as ListBox;
        var idx = list?.SelectedIndex ?? -1;
        if (idx < 0) return;

        var vm = idx < Bundles.Count ? Bundles[idx] : null;
        if (vm is null) return;
        PreviewLine = vm.Subtitle;
    }

    private static int ParseIntOrDefault(string? text, int fallback)
        => int.TryParse(text?.Trim(), out var v) ? v : fallback;

    public sealed class BundleVm
    {
        public BundleVm(string title, string subtitle)
        {
            Title = title;
            Subtitle = subtitle;
        }

        public string Title { get; }
        public string Subtitle { get; }
        public bool IsEnabled { get; set; } = true;
    }

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

