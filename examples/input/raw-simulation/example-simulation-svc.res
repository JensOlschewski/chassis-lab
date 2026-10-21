<!-- Synthetic example data for examples/config.toml. Values follow component IDs. -->
<Result>
  <Entity type="request" name="wheel_loads" entity="example.wheel_loads">
    <Component name="front_left" unitsValue="newton" id="1" />
    <Component name="front_right" unitsValue="newton" id="2" />
    <Component name="rear_left" unitsValue="newton" id="3" />
    <Component name="rear_right" unitsValue="newton" id="4" />
  </Entity>
  <Entity type="request" name="geometry" entity="example.geometry">
    <Component name="wheelbase" unitsValue="mm" id="5" />
    <Component name="front_track" unitsValue="mm" id="6" />
    <Component name="rear_track" unitsValue="mm" id="7" />
    <Component name="cg_height" unitsValue="mm" id="8" />
    <Component name="inertia_xx" unitsValue="kg*mm^2" id="9" />
    <Component name="inertia_yy" unitsValue="kg*mm^2" id="10" />
    <Component name="inertia_zz" unitsValue="kg*mm^2" id="11" />
    <Component name="front_left_wheel_center_x" unitsValue="mm" id="12" />
    <Component name="gyro_x" unitsValue="mm" id="25" />
  </Entity>
  <Entity type="request" name="ride_heights" entity="example.ride_heights">
    <Component name="front_left" unitsValue="mm" id="13" />
    <Component name="front_right" unitsValue="mm" id="14" />
    <Component name="rear_left" unitsValue="mm" id="15" />
    <Component name="rear_right" unitsValue="mm" id="16" />
  </Entity>
  <Entity type="request" name="toe_camber" entity="example.toe_camber">
    <Component name="front_left_toe" unitsValue="deg" id="17" />
    <Component name="front_right_toe" unitsValue="deg" id="18" />
    <Component name="rear_left_toe" unitsValue="deg" id="19" />
    <Component name="rear_right_toe" unitsValue="deg" id="20" />
    <Component name="front_left_camber" unitsValue="deg" id="21" />
    <Component name="front_right_camber" unitsValue="deg" id="22" />
    <Component name="rear_left_camber" unitsValue="deg" id="23" />
    <Component name="rear_right_camber" unitsValue="deg" id="24" />
  </Entity>
  <Entity type="variable" name="analysis_type" entity="example.analysis_type" entType="ADAMS_Variable">
    <Component name="Q" id="26" />
  </Entity>
  <Step type="input">
    4118.793 4020.7264999999998 3726.527 3824.5935
    2500.0 1500.0 1480.0 600.0 100000.0 200000.0 250000.0 0.0
    350.0 349.0 340.0 339.0
    0.1 -0.1 0.2 -0.2 -1.0 -1.1 -1.2 -1.3
    0.0 1
  </Step>
</Result>
