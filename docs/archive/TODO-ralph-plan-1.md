* In this simulation round, we want to achieve the following
  * Add a TUI-based dashboard that shows
    * Car's physical properties (speed, is it raining, is the car entering tunnel ((visibility 
      less), is the wiper on/off, is the headlamp on/off) along with 
      the car's logical state (Off, On, Idle, Driving, DrivingDangerously etc.)
    * DigitalTwin's internal state transition details
  * crates/common/src/observation_records contains the types and helpers for these two kinds of 
    information
  * The dashboard is going to be a standalone application. crates/tui_dashboard contains a 
    skeleton codebase for this. Dashboard will be connected to Digital Twin through a medium of 
    exchange as Zenoh and Tokio Channels.
  * The current code at crates/tui_dashboard already has the panels meant for interacting with 
    the user and displaying relevant information. We follow that.
  * We will augment the contents of each of the vertical panles later, during later rounds of 
    improvement. 