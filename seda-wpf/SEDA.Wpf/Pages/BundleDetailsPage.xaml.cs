using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Windows;
using System.Windows.Controls;
using SEDA.Wpf.Models;
using SEDA.Wpf.Services;

namespace SEDA.Wpf.Pages;

public partial class BundleDetailsPage : Page, INotifyPropertyChanged
{
    private readonly AppServices _services;

    public ObservableCollection<BundleVm> Bundles { get; } = new();
    public ObservableCollection<ActionVm> Actions { get; } = new();

    private List<RepeatedTaskBundle> _rawBundles = new();
    private List<Dictionary<string, JsonElement>> _rawActions = new();

    private string _minRepeats = "2";
    public string MinRepeats { get => _minRepeats; set => Set(ref _minRepeats, value); }

    private string _actionHeader = "Select an action";
    public string ActionHeader { get => _actionHeader; set => Set(ref _actionHeader, value); }

    private string _actionSubheader = "Details will appear here.";
    public string ActionSubheader { get => _actionSubheader; set => Set(ref _actionSubheader, value); }

    private string _actionRawJson = "";
    public string ActionRawJson { get => _actionRawJson; set => Set(ref _actionRawJson, value); }

    private string _errorLine = "";
    public string ErrorLine { get => _errorLine; set => Set(ref _errorLine, value); }

    public BundleDetailsPage(AppServices services, MainWindow shell)
    {
        InitializeComponent();
        DataContext = this;
        _services = services;
        Loaded += async (_, _) => await LoadAsync();
    }

    private async Task LoadAsync()
    {
        ErrorLine = "";
        Bundles.Clear();
        Actions.Clear();
        _rawBundles = new();
        _rawActions = new();
        ActionHeader = "Select an action";
        ActionSubheader = "Details will appear here.";
        ActionRawJson = "";

        try
        {
            var minRepeats = ParseIntOrDefault(MinRepeats, 2);
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
            var env = await _services.Api.GetRepeatedTasksAsync(minRepeats, 25, 5000, cts.Token);
            var bundles = env?.Data ?? new List<RepeatedTaskBundle>();
            _rawBundles = bundles;

            if (bundles.Count == 0)
            {
                Bundles.Add(new BundleVm("No repeated tasks yet", "Start a session and repeat a workflow 2+ times.") { IsEnabled = false });
                return;
            }

            foreach (var b in bundles)
            {
                var steps = b.Sequence?.Count ?? 0;
                var title = $"{steps} steps · x{b.Frequency}";
                var subtitle = b.SequenceLabel ?? "";
                Bundles.Add(new BundleVm(title, subtitle));
            }
        }
        catch (Exception ex)
        {
            ErrorLine = ex.Message;
        }
    }

    private async void Load_OnClick(object sender, RoutedEventArgs e) => await LoadAsync();

    private void Bundles_OnSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        Actions.Clear();
        _rawActions = new();
        ActionHeader = "Select an action";
        ActionSubheader = "Details will appear here.";
        ActionRawJson = "";

        var list = sender as ListBox;
        var idx = list?.SelectedIndex ?? -1;
        if (idx < 0 || idx >= _rawBundles.Count) return;

        var run = _rawBundles[idx].SampleRunRaw ?? new();
        _rawActions = run;

        if (run.Count == 0)
        {
            Actions.Add(new ActionVm("No actions captured", "This bundle has no sample_run.") { IsEnabled = false });
            return;
        }

        for (var i = 0; i < run.Count; i++)
        {
            var a = run[i];
            var actionType = GetString(a, "action_type") ?? "unknown";
            var app = GetString(a, "target_app") ?? GetString(a, "source_app") ?? "unknown";
            Actions.Add(new ActionVm($"{i + 1}. {actionType}", $"App: {app}"));
        }
    }

    private void Actions_OnSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        var list = sender as ListBox;
        var idx = list?.SelectedIndex ?? -1;
        if (idx < 0 || idx >= _rawActions.Count) return;

        var a = _rawActions[idx];
        var actionType = GetString(a, "action_type") ?? "unknown";
        var app = GetString(a, "target_app") ?? GetString(a, "source_app") ?? "unknown";
        var domain = GetString(a, "website_domain");
        var query = GetString(a, "search_query");
        var ts = GetString(a, "timestamp_iso");

        ActionHeader = $"{actionType} @ {app}";
        ActionSubheader = $"{(domain is not null ? $"Domain: {domain} · " : "")}{(query is not null ? $"Query: {query} · " : "")}{(ts is not null ? ts : "")}".Trim();
        ActionRawJson = JsonSerializer.Serialize(a, new JsonSerializerOptions { WriteIndented = true });
    }

    private static int ParseIntOrDefault(string? text, int fallback)
        => int.TryParse(text?.Trim(), out var v) ? v : fallback;

    private static string? GetString(Dictionary<string, JsonElement> dict, string key)
    {
        if (!dict.TryGetValue(key, out var el)) return null;
        return el.ValueKind switch
        {
            JsonValueKind.String => el.GetString(),
            JsonValueKind.Number => el.ToString(),
            JsonValueKind.True => "true",
            JsonValueKind.False => "false",
            _ => el.ToString()
        };
    }

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

    public sealed class ActionVm
    {
        public ActionVm(string title, string subtitle)
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

