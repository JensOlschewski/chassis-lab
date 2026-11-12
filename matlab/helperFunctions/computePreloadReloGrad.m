function [dfpi, dfpj, dfsi, dfsj, vPrim, vSec] = computePreloadReloGrad(v_hat_t0, jointPrimStruct_t0, jointSecStruct_t0, solverStruct_t0, signalVec_t0, varargin)
    % Computes the preload related gradient of joint forces.
    % This function calculates the gradients of primary and secondary joint
    % reaction forces based on the preload and updates to the joint and bushing
    % parameters over a specified step.
    %
    % Inputs:
    %   v_hat_t0 - Velocity vector at time t0.
    %   jointPrimStruct_t0 - Primary joint structure at time t0.
    %   jointSecStruct_t0 - Secondary joint structure at time t0.
    %   solverStruct_t0 - Solver structure at time t0.
    %   signalVec_t0 - Drive structure at time t0.
    %   currentStepIndex - Index of the current step in a simulation.
    %   varargin - Variable number of bushing structures at time t0.
    %
    % Outputs:
    %   dfpi - Gradient of primary joint reaction forces at i Part.
    %   dfpj - Gradient of primary joint reaction forces at j Part.
    %   dfsi - Gradient of secondary joint reaction forces at i Part.
    %   dfsj - Gradient of secondary joint reaction forces at j Part.


    % Stepsize for preload
    h = 1e-5;
    numB = solverStruct_t0.numB;

    % Update joint parameters for primary and secondary joints at time t1
    jointPrimStruct_t1 = updateConParam(jointPrimStruct_t0, v_hat_t0, [], h, 'kinematic');
    jointSecStruct_t1 = updateConParam(jointSecStruct_t0, v_hat_t0, [], h, 'kinematic');

    % Create joint constraints for current time step
    jointPrimStruct_t1 = fun_createJointParam(jointPrimStruct_t1);
    jointSecStruct_t1 = fun_createJointParam(jointSecStruct_t1);


    % Extract the number of input arguments
    nVarArgIn = length(varargin);

    % Check the number of input arguments
    if nVarArgIn >= 1 && nVarArgIn <= 4

        % Initialize cell arrays for bushing structures and preload forces
        bushStructs_t0 = cell(1, nVarArgIn);
        bushStructs_t1 = cell(1, nVarArgIn);
        fe_preload_t0_cell = cell(1, nVarArgIn);
        fe_preload_t1_cell = cell(1, nVarArgIn);

        % Loop through each input argument
        for i = 1:nVarArgIn
            bushStructs_t0{i} = varargin{i};
            bushStructs_t1{i} = updateConParam(bushStructs_t0{i}, v_hat_t0,[], h,'kinematic');
            bushStructs_t1{i} = fun_createConParam(bushStructs_t1{i});   
            fe_preload_t0_cell{i} = forceFromDeflection(bushStructs_t0{i},numB);
            fe_preload_t1_cell{i} = forceFromDeflection(bushStructs_t1{i},numB);
        end
        
        % Create cell array of currentLoad vectors
        load_t0 = extractActiveLoads(bushStructs_t0);
        
        for i = 1:nVarArgIn
            fe_preload_t1_cell{i} = forceFromDeflection(bushStructs_t1{i}, numB, load_t0{i});   
        end
            
        % Combine the preload vectors
        fe_preload_t0 =  signalVec_t0 + sum(cell2mat(fe_preload_t0_cell),2);
        fe_preload_t1 =  signalVec_t0 + sum(cell2mat(fe_preload_t1_cell),2);

    else
        error('Invalid number of input parameters');
    end
    
    solverStruct_t1 = createSolverStruct(solverStruct_t0, 'Force', jointPrimStruct_t1, jointSecStruct_t1);
                
    % Update preload and perform kinematic calculations for time t0 and t1
    lambda_t0 = solverStruct_t0.G_T \ -fe_preload_t0;
    lambda_t1 = solverStruct_t1.G_T \ -fe_preload_t1;
        
    % Calculate absolute joint reaction forces for time t0 and t1
    [fpi_t0, fpj_t0, fsi_t0, fsj_t0] = computeJointForces(solverStruct_t0.Gpi, solverStruct_t0.Gpj, solverStruct_t0.Gsi, solverStruct_t0.Gsj, solverStruct_t0.Tpfi, solverStruct_t0.Tpfj, solverStruct_t0.Tsfi, solverStruct_t0.Tsfj, lambda_t0);
    [fpi_t1, fpj_t1, fsi_t1, fsj_t1] = computeJointForces(solverStruct_t1.Gpi, solverStruct_t1.Gpj, solverStruct_t1.Gsi, solverStruct_t1.Gsj, solverStruct_t1.Tpfi, solverStruct_t1.Tpfj, solverStruct_t1.Tsfi, solverStruct_t1.Tsfj, lambda_t1);

    % Calculate gradient
    dfpi = (fpi_t1 - fpi_t0) ./ h;
    dfpj = (fpj_t1 - fpj_t0) ./ h;
    dfsi = (fsi_t1 - fsi_t0) ./ h;
    dfsj = (fsj_t1 - fsj_t0) ./ h;

    % Calculate velocity gradient if needed
    % -> calculate correct force always greater bodyID!!!
    dfpRelo = zeros(6*numB,1);
    dfsRelo = zeros(6*numel(jointSecStruct_t1),1);

    for i = 1:numel(jointPrimStruct_t1)
        dfpRelo((i-1)*6+1:i*6) = jointPrimStruct_t1(i).Tj_inv' * dfpj((i-1)*6+1:i*6);        
        if jointPrimStruct_t1(i).i.bodyID > jointPrimStruct_t1(i).j.bodyID
            dfpRelo((i-1)*6+1:i*6) = jointPrimStruct_t1(i).Ti_inv' * dfpi((i-1)*6+1:i*6);
        end
    end

    for i = 1:numel(jointSecStruct_t1) 
        dfsRelo((i-1)*6+1:i*6) = jointSecStruct_t1(i).Tj_inv' * dfsj((i-1)*6+1:i*6);        
    end
    vPrim = solverStruct_t0.CPrim * dfpRelo; 
    vSec = solverStruct_t0.CSecDiag * dfsRelo; 
end    

% #################
% Helper Functions
% #################
    
function loadStructs_tn = extractActiveLoads(conStructs_tn)
    % extractActiveLoads - Helper function to extract the currentLoad data
    %                      from active conStructs.
    %
    % INPUT:
    %   conStructs_tn - Cell array of structs, each containing a field 'active'
    %                   and a sub-struct 'loadStruct'.
    %
    % OUTPUT:
    %   loadStructs_tn - Cell array of currentLoad data, only if 'active' is true.

    % Initialize output structure as a cell array
    loadStructs_tn = cell(size(conStructs_tn));

    % Loop through each struct in conStructs_tn
    for i = 1:numel(conStructs_tn)
        % Access the current conStruct
        currentConStruct = conStructs_tn{i};

        % Initialize the cell array for the current conStruct
        loadStructs_tn{i} = cell(1, numel(currentConStruct));

        % Loop through each element in the current conStruct
        for n = 1:numel(currentConStruct)
            % Check if 'loadStruct' and 'active' fields exist and 'active' is true
            if isfield(currentConStruct(n), 'loadStruct') && isfield(currentConStruct(n), 'active') && currentConStruct(n).active
                % Extract currentLoad and add to the output structure
                loadStructs_tn{i}{n} = currentConStruct(n).loadStruct.currentLoad;
            else
                % If not active or fields do not exist, set to empty
                loadStructs_tn{i}{n} = [];
            end
        end
    end
end
    