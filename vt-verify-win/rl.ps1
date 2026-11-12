$tot = 0
while ($true) {
  $l = [Console]::In.ReadLine()
  if ($null -eq $l) { break }
  $tot += $l.Length
  "TOTAL=" + $tot
  if ($l -eq 'QUIT') { break }
}
