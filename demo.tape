# Charm VHS tape script for recording `yamlly` (`ymly`)
# Run with: vhs demo.tape

Output assets/demo.gif

Set FontSize 16
Set Width 1400
Set Height 820
Set Padding 18
Set Theme "Catppuccin Mocha"

Type "ymly"
Enter
Sleep 1.5s

# Open Deployment resource
Enter
Sleep 1s

# Type metadata.name
Type "payments-api"
Sleep 800ms

# Open Interactive Field Picker (+ Fields)
Ctrl+F
Sleep 1s
Type "strategy"
Sleep 1s
Escape
Sleep 600ms

# Open YAML Preview & Save modal
Ctrl+S
Sleep 2s
Escape
Sleep 600ms

# Cycle color themes
Type "t"
Sleep 1s
Type "t"
Sleep 1s
