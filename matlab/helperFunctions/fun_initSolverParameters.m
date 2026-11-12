function [solverStruct] = fun_initSolverParameters(joint_prim,joint_sec,bush_prim,bush_sec,stepParamsStruct)
    
    % Create Topology Matrices 
    [S,W] = buildInzidenzMatrix(joint_prim);
    solverStruct.S = S;
    solverStruct.W = W; 

    % Number of bodies in multibodysystem 
    solverStruct.numB = numel(joint_prim); 

    % Create Global B Matrix >> primary << 
    [~,Binv] = createBMatrix (joint_prim);
    
    % Create Global Jakobimatrix >> primary << 
    J = createJMatrix(joint_prim, Binv);

    % Create Global Constraint Matrix >> secondary <<
    Gs = createGMatrix(joint_sec, solverStruct.numB,'Gs');
    
    % Create Global closure conditions Matrix
    Gl = Gs*J;
    
    % Check if rank is correct? 
    if rank(Gl) ~= size(J,2)
        error('The multibody system is not statically determined. The number of degrees of freedom and constraints do not match.');
    end

    % Create isKin flag
    solverStruct.isKin = isKinActive(bush_prim, bush_sec);

    % Mechanism Information 
    solverStruct.numPrimConstr = solverStruct.numB*6-size(Gl,1);
    solverStruct.numSecConstr = size(Gl,1);
    solverStruct.numPrimJoints = length(joint_prim);
    solverStruct.numSecJoints = length(joint_sec);

    
    % Time parameters 
    solverStruct.t_start = stepParamsStruct.t_start;  
    solverStruct.t_end = stepParamsStruct.t_end; 
    solverStruct.h = stepParamsStruct.h;    
    solverStruct.h_preload = stepParamsStruct.h_preload; 
    [solverStruct.steps, solverStruct.time_array] = createTimeParam(solverStruct.t_start, solverStruct.t_end, solverStruct.h);
end