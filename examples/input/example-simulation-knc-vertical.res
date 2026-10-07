<!-- Synthetic vertical simulation matching example-vertical.acd corner geometry. -->
<Result>
  <Entity type="request" name="front_left" entity="front_left">
    <Component name="travel" unitsValue="mm" id="1" />
    <Component name="x" unitsValue="mm" id="2" />
    <Component name="y" unitsValue="mm" id="3" />
    <Component name="z" unitsValue="mm" id="4" />
    <Component name="tire_deflection" unitsValue="mm" id="5" />
  </Entity>
  <Entity type="request" name="front_right" entity="front_right">
    <Component name="travel" unitsValue="mm" id="6" />
    <Component name="x" unitsValue="mm" id="7" />
    <Component name="y" unitsValue="mm" id="8" />
    <Component name="z" unitsValue="mm" id="9" />
    <Component name="tire_deflection" unitsValue="mm" id="10" />
  </Entity>
  <Entity type="request" name="rear_left" entity="rear_left">
    <Component name="travel" unitsValue="mm" id="11" />
    <Component name="x" unitsValue="mm" id="12" />
    <Component name="y" unitsValue="mm" id="13" />
    <Component name="z" unitsValue="mm" id="14" />
    <Component name="tire_deflection" unitsValue="mm" id="15" />
  </Entity>
  <Entity type="request" name="rear_right" entity="rear_right">
    <Component name="travel" unitsValue="mm" id="16" />
    <Component name="x" unitsValue="mm" id="17" />
    <Component name="y" unitsValue="mm" id="18" />
    <Component name="z" unitsValue="mm" id="19" />
    <Component name="tire_deflection" unitsValue="mm" id="20" />
  </Entity>
  <Entity type="variable" name="analysis_type" entity="example.analysis_type" entType="ADAMS_Variable">
    <Component name="Q" id="21" />
  </Entity>
  <Step type="input">
    0 1200 750 280 17.0 0 1200 -750 280 17.0 0 -1200 730 285 17.0 0 -1200 -730 285 17.0 0
  </Step>
  <Step type="input">
    -1 1199.8 749.9 279 16.9 -1 1199.8 -750.1 279 16.9 -1 -1200.2 729.9 284 16.9 -1 -1200.2 -730.1 284 16.9 0
  </Step>
  <Step type="input">
    0 1200 750 280 17.0 0 1200 -750 280 17.0 0 -1200 730 285 17.0 0 -1200 -730 285 17.0 0
  </Step>
  <Step type="input">
    1 1200.2 750.1 281 17.1 1 1200.2 -749.9 281 17.1 1 -1199.8 730.1 286 17.1 1 -1199.8 -729.9 286 17.1 0
  </Step>
  <Step type="input">
    1 1200.2 750.1 281 17.1 1 1200.2 -749.9 281 17.1 1 -1199.8 730.1 286 17.1 1 -1199.8 -729.9 286 17.1 0
  </Step>
  <Step type="input">
    0 1200 750 280 17.0 0 1200 -750 280 17.0 0 -1200 730 285 17.0 0 -1200 -730 285 17.0 0
  </Step>
  <Step type="input">
    -1 1199.8 749.9 279 16.9 -1 1199.8 -750.1 279 16.9 -1 -1200.2 729.9 284 16.9 -1 -1200.2 -730.1 284 16.9 0
  </Step>
  <Step type="input">
    0 1200 750 280 17.0 0 1200 -750 280 17.0 0 -1200 730 285 17.0 0 -1200 -730 285 17.0 0
  </Step>
  <Step type="input">
    1 1200.2 750.1 281 17.1 1 1200.2 -749.9 281 17.1 1 -1199.8 730.1 286 17.1 1 -1199.8 -729.9 286 17.1 0
  </Step>
</Result>
