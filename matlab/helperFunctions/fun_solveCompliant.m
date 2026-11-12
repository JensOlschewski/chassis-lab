function historyStruct = fun_solveCompliant(jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,forceAddStruct,solverStruct,driveStruct)
% ##################################################################################
% fun_solveCompliant calls the compliance solver function
%
% This function performs a compliant (elastokinematik) analysis for a mechanical system,
% iterating over a set number of steps. In each step, it calculates joint
% matrices, solves kinematic equations, updates joint parameters, and stores
% the history of velocities and reaction forces.
%
% Inputs:
%    steps, time_array, numB
%    numJointPrim, numJointSec
%    , numBushSec
%    - Structure containing primary joint parameters
%   solverStruct - Structure containing solver parameters and settings
%   jointPrimStruct - Structure containing primary joint parameters
%   jointSecStruct - Structure containing secondary joint parameters
%   driveStruct - Structure containing drive parameters
%
% Output:
%istoryStruct - Structure containing the history of velocities and reaction forces
% ##################################################################################

% Extract solver parameters
steps = solverStruct.steps;             % Number of iteration steps
time_array = solverStruct.time_array;   % Time array
numB = solverStruct.numB;               % Number of bodies



% Preload-Step
solverStruct = createSolverStruct(solverStruct,'Kinematic',jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,forceAddStruct);
% Calculate preload forces
fePreload = forceFromDeflection(bushPrimStruct,numB) + forceFromDeflection(bushSecStruct,numB) + forceFromDeflection(forceAddStruct,numB) + driveStruct.driveForceStruct.signalVec(:,1);

lambda = solverStruct.G_T \ -fePreload;
[historyStruct.fpi, historyStruct.fpj, historyStruct.fsi, historyStruct.fsj] = computeJointForces(...
    solverStruct.Gpi, solverStruct.Gpj,...
    solverStruct.Gsi, solverStruct.Gsj,...
    solverStruct.Tpfi, solverStruct.Tpfj,...
    solverStruct.Tsfi, solverStruct.Tsfj, lambda...
    );

% Preallocate historyStruct
historyStruct.time = time_array;
historyStruct.v_hat = zeros(numB*6,steps);
historyStruct.dfpi = zeros(length(jointPrimStruct)*6,steps);
historyStruct.dfpj = zeros(length(jointPrimStruct)*6,steps);
historyStruct.dfsi = zeros(length(jointSecStruct)*6,steps);
historyStruct.dfsj = zeros(length(jointSecStruct)*6,steps);


% -- >>> Main solver iteration loop <<< ---
for i = 1:steps

    % Update progress bar
    ProgressBar(i, steps, time_array(i));

    % Update solver structure for current step
    solverStruct = createSolverStruct(solverStruct,'Compliant',jointPrimStruct,jointSecStruct,bushPrimStruct,bushSecStruct,forceAddStruct);

    % -- >>> Kinematic preStep <<< ---
    gammaEll = solverStruct.Gs * -driveStruct.driveReloStruct.driveVec(:, i);
    betaKin = solverStruct.G_ell \-gammaEll;
    v_hatKin = solverStruct.J * betaKin;

    % Relonomic incluence caused by preload and compliant mechanism --> need to check this function
    [dfpiRel, dfpjRel, dfsiRel, dfsjRel, v_hatPrimPreload, v_hatSecPreload] = computePreloadReloGrad(...
        v_hatKin, ...
        jointPrimStruct, ...
        jointSecStruct, ...
        solverStruct, ...
        driveStruct.driveForceStruct.signalVec(:, i), ...
        bushPrimStruct, ...
        bushSecStruct, ...
        forceAddStruct);

    % Calculate relonomic part from preload
    gammaPreload = solverStruct.Gs * v_hatPrimPreload -solverStruct.GsDiag *v_hatSecPreload;

    % -- >>> Main solverStep <<< ---
    % Create External Force gradient Vector
    dfe = driveStruct.driveForceStruct.driveVec(:, i);

    % Force Influence for Sec and Add Bush caused by velocity from preload from Prim Bush, Preload and Kinematic movement
    dfePreload=(solverStruct.KSec+solverStruct.KAdd)*v_hatPrimPreload;

    % Solve for kinematic variables: beta (joint velocities) and lambda (Lagrange multipliers)
    rhs = [-gammaEll-gammaPreload; -dfe-dfePreload];
    resultVector = solverStruct.solverMatrix \ rhs;
    beta = resultVector(1:solverStruct.beta_length);
    lambda = resultVector(solverStruct.beta_length+1:end);
    lambdaPrim = lambda(1:solverStruct.lambdaPrimlenght);
    lambdaSec  = lambda(solverStruct.lambdaPrimlenght+1:end);

    % Calculate kinematic part
    vHatKin=solverStruct.J*beta;

    % Calculate relative compliant velocities for conPrim
    vHatCompPrim = solverStruct.CPrim * solverStruct.GpDiag'*lambdaPrim + v_hatPrimPreload;

    % Calculate relative compliant velocities for conSec
    vHatCompSec = solverStruct.CSecDiag * solverStruct.GsDiag' * lambdaSec;

    % Calculate body velocities
    v_hat=(vHatKin + vHatCompPrim);

    % !!!! Additional Corrector Step !!!!
    % [dfpiRel, dfpjRel, dfsiRel, dfsjRel, v_hatPrimPreload, v_hatSecPreload] = computePreloadReloGrad(...
    %     v_hat, ...
    %     jointPrimStruct, ...
    %     jointSecStruct, ...
    %     solverStruct, ...
    %     driveStruct.driveForceStruct.signalVec(:, i), ...
    %     bushPrimStruct, ...
    %     bushSecStruct, ...
    %     forceAddStruct);
    %
    % gammaPreload = solverStruct.Gs * v_hatPrimPreload -solverStruct.GsDiag *v_hatSecPreload;
    % dfePreload=(solverStruct.KSec+solverStruct.KAdd)*v_hatPrimPreload;
    % rhs = [-gammaEll-gammaPreload; -dfe-dfePreload];
    % resultVector = solverStruct.solverMatrix \ rhs;
    % beta = resultVector(1:solverStruct.beta_length);
    % lambda = resultVector(solverStruct.beta_length+1:end);
    % lambdaPrim = lambda(1:solverStruct.lambdaPrimlenght);
    %
    % % Calculate kinematic part
    % vHatKin=solverStruct.J*beta;
    %
    % % Calculate relative compliant velocities from jointPrim
    % vHatCompPrim = solverStruct.CPrim * solverStruct.GpDiag'*lambdaPrim + v_hatPrimPreload;
    %
    % % Calculate body velocities
    % v_hat=(vHatKin + vHatCompPrim);

    % Calculate kinematic joint reaction forces
    [dfpi,dfpj,dfsi,dfsj] = computeJointForces(solverStruct.Gpi, solverStruct.Gpj, solverStruct.Gsi, solverStruct.Gsj, solverStruct.Tpfi, solverStruct.Tpfj, solverStruct.Tsfi, solverStruct.Tsfj, lambda);

    % Calculate connector force based on output velocity
    [dfpiComp,dfpjComp] = calculateConForces(bushPrimStruct,vHatKin,length(jointPrimStruct));
    [dfsiComp,dfsjComp] = calculateConForces(bushSecStruct,vHatKin,length(jointSecStruct));

    % Store results in history structure
    historyStruct.v_hat(:, i) = v_hat;
    historyStruct.dfpi(:, i) = dfpi + dfpiComp +  dfpiRel;
    historyStruct.dfpj(:, i) = dfpj + dfpjComp +  dfpjRel;
    historyStruct.dfsi(:, i) = dfsi + dfsiComp +  dfsiRel;
    historyStruct.dfsj(:, i) = dfsj + dfsjComp +  dfsjRel;

    % Update joint and bushing parameters
    jointPrimStruct = updateConParam(jointPrimStruct, v_hat, vHatKin, solverStruct.h,'elastic');
    jointSecStruct = updateConParam(jointSecStruct, v_hat, vHatKin, solverStruct.h,'elastic');
    bushPrimStruct = updateConParam(bushPrimStruct,v_hat, vHatCompPrim,solverStruct.h,'elastic');
    bushSecStruct = updateConParam(bushSecStruct,v_hat, vHatCompSec,solverStruct.h,'elastic');
    forceAddStruct = updateConParam(forceAddStruct,v_hat, vHatCompPrim,solverStruct.h,'elastic');

    % Assign jointPosition to bushingStruct
    bushPrimStruct = assignJointPosition(jointPrimStruct,bushPrimStruct);
    bushSecStruct = assignJointPosition(jointSecStruct,bushSecStruct);

    % Reinitialize parameters for the next iteration
    jointPrimStruct = fun_createJointParam(jointPrimStruct);
    jointSecStruct  = fun_createJointParam(jointSecStruct);
    bushPrimStruct  = fun_createConParam(bushPrimStruct);
    bushSecStruct  = fun_createConParam(bushSecStruct);
    forceAddStruct  = fun_createConParam(forceAddStruct);

end

% Cumtrapz and add preload
historyStruct.fpi = cumtrapz(historyStruct.dfpi,2)*solverStruct.h + historyStruct.fpi;
historyStruct.fpj = cumtrapz(historyStruct.dfpj,2)*solverStruct.h + historyStruct.fpj;
historyStruct.fsi = cumtrapz(historyStruct.dfsi,2)*solverStruct.h + historyStruct.fsi;
historyStruct.fsj = cumtrapz(historyStruct.dfsj,2)*solverStruct.h + historyStruct.fsj;
end
