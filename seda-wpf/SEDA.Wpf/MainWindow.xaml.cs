using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Input;
using System.Windows.Interop;
using SEDA.Wpf.Services;

namespace SEDA.Wpf;

public partial class MainWindow : Window, INotifyPropertyChanged
{
    private readonly AppServices _services;
    private bool _uiReady;

    private string _headerTitle = "Session";
    public string HeaderTitle
    {
        get => _headerTitle;
        set => Set(ref _headerTitle, value);
    }

    private string _statusText = "Starting…";
    public string StatusText
    {
        get => _statusText;
        set => Set(ref _statusText, value);
    }

    public MainWindow()
    {
        // Must be set before InitializeComponent because nav selection can fire during XAML load.
        _services = App.Services;
        InitializeComponent();
        DataContext = this;

        Loaded += MainWindow_Loaded;
        Closing += (_, __) => _services.Dispose();

        SourceInitialized += (_, _) => TryEnableWindows11Backdrop();
    }

    private void TitleBar_OnMouseLeftButtonDown(object sender, MouseButtonEventArgs e)
    {
        if (e.ClickCount >= 2)
        {
            WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;
            return;
        }
        DragMove();
    }

    private void Minimize_OnClick(object sender, RoutedEventArgs e) => WindowState = WindowState.Minimized;
    private void Maximize_OnClick(object sender, RoutedEventArgs e)
        => WindowState = WindowState == WindowState.Maximized ? WindowState.Normal : WindowState.Maximized;
    private void Close_OnClick(object sender, RoutedEventArgs e) => Close();

    private async void MainWindow_Loaded(object sender, RoutedEventArgs e)
    {
        _uiReady = true;
        try
        {
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10));
            var ok = await _services.Launcher.EnsureRunningAsync(cts.Token);
            StatusText = ok ? "Idle" : "Backend failed (see seda-wpf/README.md)";
            NavList.SelectedIndex = 0;
            NavigateTo(0);
        }
        catch (Exception ex)
        {
            StatusText = $"Error: {ex.Message}";
        }
    }

    private void TryEnableWindows11Backdrop()
    {
        try
        {
            // Enable immersive dark mode title bar (Win 10 1809+; best-effort).
            var hwnd = new WindowInteropHelper(this).Handle;
            if (hwnd == IntPtr.Zero) return;

            const int DWMWA_USE_IMMERSIVE_DARK_MODE = 20;
            var useDark = 1;
            DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, ref useDark, sizeof(int));

            // Try Mica (Windows 11). If unsupported, it will no-op.
            // DWMWA_SYSTEMBACKDROP_TYPE: 38
            // 2 = Mica, 3 = Acrylic (varies), 4 = Tabbed
            const int DWMWA_SYSTEMBACKDROP_TYPE = 38;
            var backdropType = 2;
            DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, ref backdropType, sizeof(int));
        }
        catch
        {
            // best-effort; app still works without it
        }
    }

    [DllImport("dwmapi.dll", PreserveSig = true)]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int attrValue, int attrSize);

    private void NavList_OnSelectionChanged(object sender, System.Windows.Controls.SelectionChangedEventArgs e)
    {
        if (!_uiReady)
            return;
        NavigateTo(NavList.SelectedIndex);
    }

    private void NavigateTo(int navIndex)
    {
        if (navIndex < 0) navIndex = 0;
        switch (navIndex)
        {
            case 0:
                HeaderTitle = "Session";
                RootFrame.Navigate(new Pages.SessionPage(_services, this));
                break;
            case 1:
                HeaderTitle = "Repeated Tasks";
                RootFrame.Navigate(new Pages.RepeatedTasksPage(_services, this));
                break;
            case 2:
                HeaderTitle = "Bundle Details";
                RootFrame.Navigate(new Pages.BundleDetailsPage(_services, this));
                break;
        }
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

