function outputStruct = initOutputStruct(jsonStruct)
    % ##################################################################################
    % Function: initOutputStruct
    % Description: Initializes the output structure based on JSON configuration input, 
    %              specifying which data (e.g., body, joint, bushing) should be 
    %              included in the output.
    %
    % Inputs:
    %   jsonStruct - JSON-based structure containing configuration flags for output data.
    %
    % Output:
    %   outputStruct - Initialized structure indicating included data types.
    % ##################################################################################
    
    outputStruct = struct();

    outputStruct.isBodyOutput = jsonStruct.include_body_data;
    outputStruct.isConOutput = jsonStruct.include_connector_data;
    outputStruct.isSuspensionParamOutput = jsonStruct.include_suspension_parameter; 

    outputStruct.wheelLeft.wcName = jsonStruct.parameters.left_wheel.wc_name;
    outputStruct.wheelLeft.wnName = jsonStruct.parameters.left_wheel.wn_name;
    outputStruct.wheelLeft.wpName = jsonStruct.parameters.left_wheel.wp_name;
    outputStruct.wheelLeft.bodyID = jsonStruct.parameters.left_wheel.bodyID;
    outputStruct.wheelLeft.jointID = jsonStruct.parameters.left_wheel.wheelpatch_jointID;
    outputStruct.wheelLeft.jointTopology = jsonStruct.parameters.left_wheel.wheelpatch_jointTopology;
    outputStruct.wheelRight.wcName = jsonStruct.parameters.right_wheel.wc_name;
    outputStruct.wheelRight.wnName = jsonStruct.parameters.right_wheel.wn_name;
    outputStruct.wheelRight.wpName = jsonStruct.parameters.right_wheel.wp_name;
    outputStruct.wheelRight.bodyID = jsonStruct.parameters.right_wheel.bodyID;
    outputStruct.wheelRight.jointID = jsonStruct.parameters.right_wheel.wheelpatch_jointID;
    outputStruct.wheelRight.jointTopology = jsonStruct.parameters.right_wheel.wheelpatch_jointTopology;
if isfield(jsonStruct.parameters,'left_spring') 
    outputStruct.springLeft.suName = jsonStruct.parameters.left_spring.su_name;
    outputStruct.springLeft.slName = jsonStruct.parameters.left_spring.sl_name;
    outputStruct.springLeft.suBodyID = jsonStruct.parameters.left_spring.bodyID_su;
    outputStruct.springLeft.slBodyID = jsonStruct.parameters.left_spring.bodyID_sl;
end

if isfield(jsonStruct.parameters,'right_spring') 
    outputStruct.springRight.suName = jsonStruct.parameters.right_spring.su_name;
    outputStruct.springRight.slName = jsonStruct.parameters.right_spring.sl_name;
    outputStruct.springRight.suBodyID = jsonStruct.parameters.right_spring.bodyID_su;
    outputStruct.springRight.slBodyID = jsonStruct.parameters.right_spring.bodyID_sl;
end

if isfield(jsonStruct.parameters,'left_damper') 
    outputStruct.damperLeft.duName = jsonStruct.parameters.left_damper.du_name;
    outputStruct.damperLeft.dlName = jsonStruct.parameters.left_damper.dl_name;
    outputStruct.damperLeft.duBodyID = jsonStruct.parameters.left_damper.bodyID_du;
    outputStruct.damperLeft.dlBodyID = jsonStruct.parameters.left_damper.bodyID_dl;
end

if isfield(jsonStruct.parameters,'right_damper') 
    outputStruct.damperRight.duName = jsonStruct.parameters.right_damper.du_name;
    outputStruct.damperRight.dlName = jsonStruct.parameters.right_damper.dl_name;
    outputStruct.damperRight.duBodyID = jsonStruct.parameters.right_damper.bodyID_du;
    outputStruct.damperRight.dlBodyID = jsonStruct.parameters.right_damper.bodyID_dl;
end
