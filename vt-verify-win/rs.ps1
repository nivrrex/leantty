while ($true) {
  $l = [Console]::In.ReadLine()
  if ($null -eq $l) { break }
  [BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($l))
  if ($l -eq 'QUIT') { break }
}
