Output docs/assets/compare-knc.gif

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 900
Set Theme "Dracula"

Type "./target/debug/clab compare examples/config.toml examples/run-measurement.toml examples/run-simulation.toml --tolerances | sed -n '/^KnC Vertical$/,$p'"
Enter
Sleep 3s
