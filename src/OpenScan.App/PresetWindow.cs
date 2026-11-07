using OpenScan.Core;

namespace OpenScan.App;

internal sealed class PresetWindow : Form
{
    private readonly PresetStore store;
    private readonly IScannerBackend backend;
    private readonly ListBox list = new() { Dock = DockStyle.Fill, DisplayMember = nameof(ScanProfile.Name), IntegralHeight = false };
    private readonly Label details = new() { Dock = DockStyle.Bottom, Height = 105, Padding = new Padding(12), BackColor = Color.FromArgb(244, 247, 250) };

    public PresetWindow(PresetStore store, IScannerBackend backend)
    {
        this.store = store; this.backend = backend;
        Text = "Manage presets — OpenScan"; Size = new Size(640, 490); MinimumSize = new Size(600, 400);
        StartPosition = FormStartPosition.CenterParent; Font = new Font("Segoe UI", 10);
        var title = new Label { Text = "Scanner presets", Dock = DockStyle.Top, Height = 58, Font = new Font(Font.FontFamily, 17, FontStyle.Bold), Padding = new Padding(12) };
        var buttons = new FlowLayoutPanel { Dock = DockStyle.Bottom, Height = 58, Padding = new Padding(8) };
        buttons.Controls.Add(SettingsControl.Button("New", () => Edit(null)));
        buttons.Controls.Add(SettingsControl.Button("Edit", () => { if (Selected is { } p) Edit(p); }));
        buttons.Controls.Add(SettingsControl.Button("Duplicate", Duplicate));
        buttons.Controls.Add(SettingsControl.Button("Rename", Rename));
        buttons.Controls.Add(SettingsControl.Button("Delete", Delete));
        buttons.Controls.Add(SettingsControl.Button("Close", Close));
        Controls.Add(list); Controls.Add(details); Controls.Add(buttons); Controls.Add(title);
        list.SelectedIndexChanged += (_, _) =>
        {
            if (Selected is not { } p) return;
            details.Text = $"{p.Source}  •  {p.Color}  •  {p.Paper}  •  {p.Dpi} DPI\nDeskew: {p.Cleanup.Deskew}   Crop: {p.Cleanup.AutoCrop}   Skip blank: {p.Cleanup.SkipBlankPages}\nSearchable PDF: {p.SearchablePdf}   JPEG quality: {p.JpegQuality}";
        };
        list.DoubleClick += (_, _) => { if (Selected is { } p) Edit(p); };
        RefreshList();
    }
    private ScanProfile? Selected => list.SelectedItem as ScanProfile;
    private void RefreshList(string? select = null)
    {
        try
        {
            list.Items.Clear(); foreach (var p in store.List()) list.Items.Add(p);
            list.SelectedItem = list.Items.Cast<ScanProfile>().FirstOrDefault(x => x.Name == select);
            if (list.SelectedIndex < 0 && list.Items.Count > 0) list.SelectedIndex = 0;
        }
        catch (Exception e) { MessageBox.Show(this, e.Message, "Cannot read presets"); }
    }
    private void Edit(ScanProfile? existing)
    {
        using var dialog = new Form { Text = existing == null ? "New preset" : "Edit preset", Size = new Size(420, 800), MinimumSize = new Size(420, 500), StartPosition = FormStartPosition.CenterParent, Font = Font };
        var settings = new SettingsControl(backend);
        settings.Apply(existing ?? new ScanProfile());
        var footer = new FlowLayoutPanel { Dock = DockStyle.Bottom, Height = 55, Padding = new Padding(8) };
        footer.Controls.Add(SettingsControl.Button("Save preset", () =>
        {
            string? name = existing?.Name ?? NameDialog.Ask(dialog, "Preset name", "New preset");
            if (name == null) return;
            try { store.Save(settings.Read(name), existing != null); dialog.DialogResult = DialogResult.OK; dialog.Close(); RefreshList(name); }
            catch (Exception e) { MessageBox.Show(dialog, e.Message, "Cannot save preset"); }
        }));
        footer.Controls.Add(SettingsControl.Button("Cancel", dialog.Close));
        dialog.Controls.Add(settings); dialog.Controls.Add(footer);
        dialog.Shown += async (_, _) => await settings.RefreshDevices();
        dialog.ShowDialog(this);
    }
    private void Duplicate()
    {
        if (Selected is not { } p) return;
        string? name = NameDialog.Ask(this, "Duplicate preset", p.Name + " copy"); if (name == null) return;
        try { store.Save(p with { Name = name }); RefreshList(name); } catch (Exception e) { MessageBox.Show(this, e.Message); }
    }
    private void Rename()
    {
        if (Selected is not { } p) return;
        string? name = NameDialog.Ask(this, "Rename preset", p.Name); if (name == null || name == p.Name) return;
        try
        {
            if (name.Equals(p.Name, StringComparison.OrdinalIgnoreCase)) store.Save(p with { Name = name }, true);
            else { store.Save(p with { Name = name }); store.Delete(p.Name); }
            RefreshList(name);
        }
        catch (Exception e) { MessageBox.Show(this, e.Message); }
    }
    private void Delete()
    {
        if (Selected is not { } p || MessageBox.Show(this, $"Delete preset '{p.Name}'?", "Delete preset", MessageBoxButtons.YesNo) != DialogResult.Yes) return;
        try { store.Delete(p.Name); RefreshList(); } catch (Exception e) { MessageBox.Show(this, e.Message); }
    }
}

internal static class NameDialog
{
    public static string? Ask(IWin32Window owner, string title, string initial)
    {
        using var form = new Form { Text = title, Size = new Size(400, 155), StartPosition = FormStartPosition.CenterParent, FormBorderStyle = FormBorderStyle.FixedDialog, MaximizeBox = false, MinimizeBox = false };
        var text = new TextBox { Text = initial, Dock = DockStyle.Top, MaxLength = 80, Margin = new Padding(12) };
        var buttons = new FlowLayoutPanel { Dock = DockStyle.Bottom, Height = 45 };
        var save = SettingsControl.Button("OK", () => { if (!string.IsNullOrWhiteSpace(text.Text)) form.DialogResult = DialogResult.OK; });
        buttons.Controls.Add(save); buttons.Controls.Add(SettingsControl.Button("Cancel", () => form.DialogResult = DialogResult.Cancel));
        form.Controls.Add(text); form.Controls.Add(buttons); form.Padding = new Padding(12); form.AcceptButton = save;
        return form.ShowDialog(owner) == DialogResult.OK ? text.Text.Trim() : null;
    }
}
