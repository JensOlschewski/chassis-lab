function [historyStruct,bushAddStruct] = fun_solvePreloadKinematic(driveStruct, bushAddStruct, jointPrimStruct, jointSecStruct, solverStruct)
    % fun_solvePreloadKinematic Solves the preload kinematics for a mechanical system.
    %
    % This function calculates the joint reaction forces in a mechanical system
    % under preload conditions. It considers both external preload forces specified
    % in 'driveStruct' and additional preload forces from 'bushAddStruct'. The function 
    % computes the joint parameters, performs kinematic precalculations, and solves 
    % for preload forces. Finally, it calculates absolute joint reaction forces.
    %
    % Input:
    %   driveStruct     - A struct containing drive parameters and preload vector.
    %   bushAddStruct   - An array of structs representing additional bush elements.
    %   jointPrimStruct - A struct with primary joint parameters.
    %   jointSecStruct  - A struct with secondary joint parameters.
    %   solverStruct    - A struct with solver parameters.
    %
    % Output:
    %   historyStruct   - A struct containing calculated joint reaction forces.
        
    % Check for preload conditions and calculate preload vectors
    if driveStruct.ispreload || any([bushAddStruct.isPreload])
        
        % Calculate preload vectors
        fe = driveStruct.driveForceStruct.signalVec(:,1);  

        % Assign Current Preload to BushStruct
        [bushAddStruct.preloadCurrent] = deal(bushAddStruct.preloads);

        % Create Preload Vector
        fe_preload = createPreloadVec(fe, solverStruct.numB, bushAddStruct);

        % Create Variables in solverStruct
        solverStruct = createSolverStruct(solverStruct,'Force',jointPrimStruct,jointSecStruct);
    
        % Solve for preload 
        lambda = solverStruct.G_T\-fe_preload;
    
        % Calculate absolute joint reaction forces
        [fpi, fpj, fsi, fsj] = computeJointForces(solverStruct.Gpi, solverStruct.Gpj, solverStruct.Gsi, solverStruct.Gsj, solverStruct.Tpfi, solverStruct.Tpfj, solverStruct.Tsfi, solverStruct.Tsfj, lambda);
    
    else 
        numSecJoints = length(jointSecStruct);  % Number of joints
        fpi = zeros(6 * solverStruct.numB, 1);
        fpj = zeros(6 * solverStruct.numB, 1);
        fsi = zeros(6 * numSecJoints, 1);
        fsj = zeros(6 * numSecJoints, 1);
    end

    % Store results in historyStruct
    historyStruct.fpi = fpi; 
    historyStruct.fpj = fpj; 
    historyStruct.fsi = fsi; 
    historyStruct.fsj = fsj; 
end
