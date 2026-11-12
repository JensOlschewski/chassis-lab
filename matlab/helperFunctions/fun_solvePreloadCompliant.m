function [historyStruct,jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,bushAddStruct] = fun_solvePreloadCompliant(jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,bushAddStruct,driveStruct,solverStruct)
 % fun_solvePreloadCompliant Solves the preload kinematics for a mechanical system.
    %
    % This function evaluates the dynamic behavior of a mechanical system under
    % preload conditions, with an emphasis on joint reaction forces. It modularly handles 
    % the processing of preload conditions, solving kinematic equations, and computing 
    % joint reaction forces. The function starts by checking for preload conditions and 
    % preparing necessary parameters. It then iterates through time stepsPre to solve 
    % kinematics and compute joint forces, updating preload and kinematic parameters 
    % at each step. Finally, it compiles the results into a history structure.
    %
    % Input:
    %   jointPrimStruct - A struct with primary joint parameters.
    %   jointSecStruct  - A struct with secondary joint parameters.
    %   bushPrimStruct  - A struct with primary bush parameters.
    %   bushSecStruct   - A struct with secondary bush parameters.
    %   bushAddStruct   - An array of structs for additional bush elements.
    %   driveStruct     - A struct containing drive parameters and preload vector.
    %   solverStruct    - A struct with solver parameters for kinematic equations.
    %
    % Output:
    %   historyStruct   - A struct containing the calculated joint reaction forces 
    %                     and other relevant kinematic data over the preload period.

    % Defined preload time parameters
    t0Pre = 0; % FIXED Not defined by User
    tendPre = 1;   % FIXED Not defined by User
    hPre = solverStruct.h_preload; 
    [stepsPre, timePre] = createTimeParam(t0Pre, tendPre, hPre);

    % Approach 
    % Absolute Force at time t0 
    % Apply Total Force -> Better Gradient  
    % Calculate Deflection 
    % Rheonomic Force Part 
    % New Absolute Force by new Position 
    % Compare currentForce with New Absolute Force

    % Force divided into ten segments  

    % Create Preload Signals 
    [preloadSignal, preloadGrad] = createPreloadArrays(driveStruct.driveForceStruct.signalVec (:,1), stepsPre, hPre,'linear');
    [bushPrimPreloadSigStruct, primPreGrad] = createBushPreloadStruct(bushPrimStruct,stepsPre,hPre,'linear');
    [bushSecPreloadSigStruct, secPreGrad]  = createBushPreloadStruct(bushSecStruct,stepsPre,hPre,'linear');
    [bushAddPreloadSigStruct, addPreGrad]  = createBushPreloadStruct(bushAddStruct,stepsPre,hPre,'linear');

    for i = 1:stepsPre
        % Extract Preload Gradient for current timestep
        extPrimGrad = extractColumnFromStruct(primPreGrad, i);
        extSecGrad = extractColumnFromStruct(secPreGrad, i);
        extAddGrad = extractColumnFromStruct(addPreGrad, i);

        % Create combined PreloadGradientVector
        dfe_preload = createPreloadVec(preloadGrad(:,i),solverStruct.numB,bushPrimStruct,extPrimGrad,bushSecStruct,extSecGrad,bushAddStruct,extAddGrad);
        
        % Create Joint parameters for current time step
        jointPrimStruct = fun_createJointParam(jointPrimStruct);
        jointSecStruct = fun_createJointParam(jointSecStruct);
        
        % Create BushAdd parameter for current time step 
        bushPrimStruct = fun_createBushParam(bushPrimStruct);
        bushSecStruct = fun_createBushParam(bushSecStruct);
        bushAddStruct = fun_createBushParam(bushAddStruct);
        
        % Create Variables in solverStruct
        solverStruct = createSolverStruct(solverStruct,'Compliant',jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,bushAddStruct);
        
        % Create Relonomic Parameters
        gamma_l = solverStruct.Gs*zeros(solverStruct.numB*6,1);

        % Solve for beta and lambda
        result_vector = solverStruct.solverMatrix \ [-gamma_l; -dfe_preload];
        beta = result_vector(1:solverStruct.beta_length);
        lambda = result_vector(solverStruct.beta_length+1:end);

        % Compute Joint Forces
        [dfpi,dfpj,dfsi,dfsj] = computeJointForces(solverStruct.Gpi, solverStruct.Gpj, solverStruct.Gsi, solverStruct.Gsj, solverStruct.Tpfi, solverStruct.Tpfj, solverStruct.Tsfi, solverStruct.Tsfj, lambda);
        
        % Calculate kinematic body velocities 
        v_hat_kin = solverStruct.J*beta; 

        % Calculate overall body velocities
        v_hat=v_hat_kin+solverStruct.CPrim*solverStruct.GpDiag'*lambda(1:solverStruct.lambdaPrimlenght); 
        
        % Preload incluence 
        [dfpiRel,dfpjRel,dfsiRel,dfsjRel,vprimRel,vsecRel] = computePreloadReloGrad(v_hat,jointPrimStruct,jointSecStruct,solverStruct,preloadSignal(:,i),'ForceAndVelocity',bushPrimStruct,bushSecStruct,bushAddStruct,bushFlexStruct);    
        
        % Calculate relative compliant velocities
        % v_hat_comp_sec_relative = solverStruct.CSecDiag * solverStruct.GsDiag'*lambda(solverStruct.lambdaPrimlenght+1:end);

        % Relonomic Influence caused by Preload and compliance of PrimBushing
        gamma_preload = solverStruct.Gs * vprimRel -solverStruct.GsDiag * vsecRel;

        % Force Influence caused by Preload and compliance of PrimBushing
        dfe_preload_relo=(solverStruct.KSec+solverStruct.KAdd)*vprimRel;

        result_vector = solverStruct.solverMatrix \ [(-gamma_l-gamma_preload); -dfe_preload-dfe_preload_relo];
        beta = result_vector(1:solverStruct.beta_length);
        lambda = result_vector(solverStruct.beta_length+1:end);

        % % Calculate kinematic body velocities 
        v_hat_kin = solverStruct.J*beta; 
        
        % % 
        v_hat=solverStruct.J*beta+solverStruct.CPrim*solverStruct.GpDiag'*lambda(1:solverStruct.lambdaPrimlenght); 

        % % % % Calculate relative compliant velocities
        v_hat_comp_prim_relative = solverStruct.CPrimDiag * solverStruct.GpDiag'*lambda(1:solverStruct.lambdaPrimlenght); 

        %  Corrector Step Relonomic Part
        %[dfpiRel,dfpjRel,dfsiRel,dfsjRel,~,~] = computePreloadReloGrad(v_hat,jointPrimStruct,jointSecStruct,solverStruct,preloadSignal(:,i),'ForceAndVelocity',bushPrimStruct,bushSecStruct,bushAddStruct);    
        
        % Calculate preloads based on output Velocity 
        if i ~=stepsPre
        [bushPrimStruct,dfpiComp,dfpjComp] = updateBushPreloads(bushPrimStruct,v_hat_kin,hPre,length(jointPrimStruct));
        [bushSecStruct,dfsiComp,dfsjComp] = updateBushPreloads(bushSecStruct,v_hat,hPre,length(jointSecStruct));
        [bushAddStruct,~,~] = updateBushPreloads(bushAddStruct,v_hat,hPre, []);

        % Update joint parameters for primary and secondary joints
        jointPrimStruct = updateConnParam(jointPrimStruct, v_hat, [], hPre);
        jointSecStruct = updateConnParam(jointSecStruct, v_hat, [], hPre);
            
        % Overwrite absolute preload values from Signals when isPreload = true
        bushPrimStruct = assignPreloadToBush (i+1,bushPrimStruct,bushPrimPreloadSigStruct); 
        bushSecStruct = assignPreloadToBush (i+1,bushSecStruct,bushSecPreloadSigStruct); 
        bushAddStruct = assignPreloadToBush (i+1,bushAddStruct,bushAddPreloadSigStruct);
        bushFlexStruct = assignPreloadToBush (i+1,bushFlexStruct,bushFlexPreloadSigStruct);
                
        % Update bushing parameters 
        bushPrimStruct = updateConnParam(bushPrimStruct,v_hat, [],hPre);
        bushSecStruct = updateConnParam(bushSecStruct,v_hat, [], hPre);
        bushAddStruct = updateConnParam(bushAddStruct,v_hat, [],hPre);
        bushFlexStruct = updateConnParam(bushFlexStruct,v_hat, [],hPre);

        else
            [~,dfpiComp,dfpjComp] = updateBushPreloads(bushPrimStruct,v_hat_kin,hPre,length(jointPrimStruct));
            [~,dfsiComp,dfsjComp] = updateBushPreloads(bushSecStruct,v_hat,hPre,length(jointSecStruct));
        end

        % Store results in history structure
        historyStruct.v_hatPreload(:, i) = v_hat;
        historyStruct.dfpiPreload(:, i) = dfpi + dfpiComp+ dfpiRel;
        historyStruct.dfpjPreload(:, i) = dfpj + dfpjComp+ dfpjRel;
        historyStruct.dfsiPreload(:, i) = dfsi + dfsiComp+ dfsiRel;
        historyStruct.dfsjPreload(:, i) = dfsj + dfsjComp+ dfsjRel;
    end
    % Cumtrapz and add preload
    historyStruct.fpiPreload = cumtrapz(historyStruct.dfpiPreload,2)*hPre; 
    historyStruct.fpjPreload = cumtrapz(historyStruct.dfpjPreload,2)*hPre; 
    historyStruct.fsiPreload = cumtrapz(historyStruct.dfsiPreload,2)*hPre; 
    historyStruct.fsjPreload = cumtrapz(historyStruct.dfsjPreload,2)*hPre; 
    historyStruct.timePreload = timePre;

    % Store results in historyStruct
    historyStruct.fpi = historyStruct.fpiPreload(:,end);
    historyStruct.fpj = historyStruct.fpjPreload(:,end);
    historyStruct.fsi = historyStruct.fsiPreload(:,end); 
    historyStruct.fsj = historyStruct.fsjPreload(:,end); 
end
