class_name SceneRegistry
extends RefCounted
## SceneRegistry - Centralized preloading of screen scenes.
##
## This file is the Single Source of Truth (SSoT) for screen scene preloads.
## Screens are preloaded for instant transitions.
##
## UI components now build their UI in code and should be instantiated
## directly using ComponentClass.new() (e.g., Card.new(), Divider.new()).

# Screen scenes (preloaded for instant transitions)
const SetupScreen := preload("res://src/ui/screens/setup/setup_screen.tscn")
const FocusScreen := preload("res://src/ui/screens/focus/focus_screen.tscn")
const StimulusScreen := preload("res://src/ui/screens/stimulus/stimulus_screen.tscn")
const AcquireScreen := preload("res://src/ui/screens/run/run_screen.tscn")
const ResultsScreen := preload("res://src/ui/screens/analyze/analyze_screen.tscn")
