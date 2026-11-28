
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'MITO - Selecciona la carpeta del host para enlazar'
$dialog.ShowNewFolderButton = $false
$dialog.SelectedPath = [Console]::In.ReadToEnd()
try {
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        ConvertTo-Json -InputObject $dialog.SelectedPath -Compress
    }
} finally { $dialog.Dispose() }
