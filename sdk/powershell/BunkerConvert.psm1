function Invoke-BunkerConvertRecipe {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string] $RecipePath,

        [string] $Binary = "bunker-convert",

        [string[]] $AdditionalArguments = @(),

        [switch] $PassThru
    )

    $arguments = @("run", (Resolve-Path -LiteralPath $RecipePath).Path)
    if ($AdditionalArguments) {
        $arguments += $AdditionalArguments
    }

    $stderrPath = [System.IO.Path]::GetTempFileName()
    $params = @{
        FilePath = $Binary
        ArgumentList = $arguments
        NoNewWindow = $true
        Wait = $true
        PassThru = $true
        RedirectStandardError = $stderrPath
    }

    if (-not $PassThru.IsPresent) {
        $params.RedirectStandardOutput = [System.IO.Path]::GetTempFileName()
    }

    $process = Start-Process @params

    if ($process.ExitCode -ne 0) {
        $stderr = Get-Content $stderrPath
        throw "bunker-convert failed with code $($process.ExitCode): $stderr"
    }

    if ($PassThru) {
        Remove-Item $stderrPath -ErrorAction SilentlyContinue
        return $process
    }

    $stdoutPath = $params.RedirectStandardOutput
    $output = Get-Content $stdoutPath
    Remove-Item $stdoutPath, $stderrPath -ErrorAction SilentlyContinue
    $output
}

function Invoke-BunkerConvertRecipeLint {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]
        [string[]] $RecipePaths,

        [string] $Binary = "bunker-convert"
    )

    $resolved = $RecipePaths | ForEach-Object { (Resolve-Path -LiteralPath $_).Path }
    $stdoutPath = [System.IO.Path]::GetTempFileName()
    $stderrPath = [System.IO.Path]::GetTempFileName()

    $process = Start-Process -FilePath $Binary -ArgumentList @("recipe", "lint") + $resolved -NoNewWindow -Wait -PassThru -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath

    if ($process.ExitCode -ne 0) {
        $stderr = Get-Content $stderrPath
        throw "bunker-convert lint failed with code $($process.ExitCode): $stderr"
    }

    $output = Get-Content $stdoutPath
    Remove-Item $stdoutPath, $stderrPath -ErrorAction SilentlyContinue
    $output
}

Export-ModuleMember -Function Invoke-BunkerConvertRecipe, Invoke-BunkerConvertRecipeLint
