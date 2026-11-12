function solverStruct = createSolverStruct(solverStruct,caseString,varargin)
    % ##################################################################################
    % CREATE SOLVER STRUCT
    % This function creates and returns a solver structure based on the specified case.
    % It supports different types of cases such as 'Force' and 'Kinematic'. 
    % Each case processes inputs differently to populate the solver structure.
    %
    % Inputs:
    %   caseString - A string specifying the case type (e.g., 'Force', 'Kinematic').
    %   varargin - A variable number of input arguments depending on the case type.
    %
    % Outputs:
    %   solverStruct - A structure containing solver parameters specific to the case type.
    % ##################################################################################


    switch caseString
        case 'Compliant'
            % Input Joint and BushStructs  
            jointPrimStruct = varargin{1};
            jointSecStruct = varargin{2};
            bushPrimStruct =varargin{3};
            bushSecStruct = varargin{4};
            forceAddStruct = varargin{5};

            % Determine number of joints
            numPrimJoints = length(jointPrimStruct);
            numSecJoints = length(jointSecStruct);

            % Kinematic Paramaters 
            [Binv,Gpi,Gpj,Gsi,Gsj,Tpfi,Tpfj,Tsfi,Tsfj,J,GpDiag,Gs,GsDiag,GsDiagMod] = createKinematicSolverParams(jointPrimStruct, jointSecStruct, numPrimJoints, numSecJoints);
            
            % Compliant Parameters 
            KPrim = createGlobKMat(bushPrimStruct,'absolute',solverStruct.numB);
            KSec = createGlobKMat(bushSecStruct,'absolute',solverStruct.numB);
            KAdd = createGlobKMat(forceAddStruct,'absolute',solverStruct.numB);
            KSecRelative = createGlobKMat(bushSecStruct,'relative',solverStruct.numB,numSecJoints);
            
            CPrimDiag = createGlobCMat(bushPrimStruct,numPrimJoints);
            CSecDiag = createGlobCMat(bushSecStruct,numSecJoints);
            CPrim = Binv * CPrimDiag;

            % Create constraint Jakobian Matrix >> Gell << 
            G_ell = Gs * J;
            % Create global Implicit constraint Matrix >> G_T <<
            G_T = [Binv'\GpDiag', Gs'];
            % Create global coupled compliance Matrix C_ell
            C_ell = [Gs*CPrim*GpDiag',-GsDiag*CSecDiag*GsDiagMod'];
            % Determine size of Result vector
            beta_length = size (G_ell,1); 
            lambdaPrimlenght = size(GpDiag,1);
            % Create stiffness Jakobian Matrix >> KJ << 
            KJ = (KPrim+KSec+KAdd)*J;
            % Create stiffness Matrix for Lambda Influence
            KLambda=(KSec+KAdd)*CPrim*GpDiag';
            KLambda = [KLambda,zeros(solverStruct.numB*6,beta_length)];
            
            % Create Solver Matrix 
            solverMatrix = [...
                G_ell,  C_ell;... 
                KJ,     KLambda+G_T...
                ];

            % Assign results to solverStruct
            solverStruct=assignSolverStructFieldsDynamic(solverStruct,...
            "Gpi",Gpi,"Gpj",Gpj,'Gsi',Gsi,'Gsj',Gsj,...
            "Tpfi",Tpfi,"Tpfj",Tpfj,'Tsfi',Tsfi,'Tsfj',Tsfj,...
            "J",J,"G_T",G_T,'G_ell',G_ell,...
            "GpDiag",GpDiag,"GsDiag",GsDiag,'Gs',Gs,...
            "CPrim",CPrim,"CPrimDiag",CPrimDiag,"CSecDiag",CSecDiag,'Cell', C_ell, ...
            "KJ",KJ,"KSec",KSec,'KAdd',KAdd,"KSecRelative",KSecRelative,"beta_length",beta_length,'lambdaPrimlenght',lambdaPrimlenght,...
            'solverMatrix',solverMatrix);
            
        case 'Force'
            jointPrimStruct = varargin{1};
            jointSecStruct = varargin{2};
            numPrimJoints = length(jointPrimStruct);
            numSecJoints = length(jointSecStruct);

            % Kinematic Paramaters 
            [Binv,Gpi,Gpj,Gsi,Gsj,Tpfi,Tpfj,Tsfi,Tsfj,J,GpDiag,Gs,~] = createKinematicSolverParams(jointPrimStruct, jointSecStruct, numPrimJoints, numSecJoints);        

            % Create global Implicit constraint Matrix >> G_T <<
            G_T = [Binv'\GpDiag', Gs'];
    
            % Assign results to solverStruct
            solverStruct.Gpi = Gpi;
            solverStruct.Gpj = Gpj;
            solverStruct.Gsi = Gsi;
            solverStruct.Gsj = Gsj;
            solverStruct.Tpfi = Tpfi;
            solverStruct.Tpfj = Tpfj;
            solverStruct.Tsfi = Tsfi;
            solverStruct.Tsfj = Tsfj;
            solverStruct.Tsfj = Tsfj;
            solverStruct.J = J;
            solverStruct.Gp = GpDiag;
            solverStruct.Gs = Gs;
            solverStruct.G_T = G_T;


        case 'Kinematic'
            % Assign JointStructs
            jointPrimStruct = varargin{1};
            jointSecStruct = varargin{2};

            % Determine number of joints
            numPrimJoints = numel(jointPrimStruct);
            numSecJoints = numel(jointSecStruct);

            % Kinematic Parameters
            [Binv,Gpi,Gpj,Gsi,Gsj,Tpfi,Tpfj,Tsfi,Tsfj,J,GpDiag,Gs,~] = createKinematicSolverParams(jointPrimStruct, jointSecStruct, numPrimJoints, numSecJoints);        

            % Create constraint Jakobian Matrix >> Gell << 
            Gell = Gs * J;

            % Create global Implicit constraint Matrix >> G_T <<
            G_T = [Binv'\GpDiag', Gs'];

            % Determine size of Result vectors
            beta_length = size (Gell,1); 
            lambda_length = size (G_T,1);

            % Create Solver Matrix
            solverMatrix = [Gell,zeros(beta_length,lambda_length); zeros(lambda_length,beta_length),G_T];

            % Assign results to solverStruct
            solverStruct.Gpi = Gpi;
            solverStruct.Gpj = Gpj;
            solverStruct.Gsi = Gsi;
            solverStruct.Gsj = Gsj;
            solverStruct.Tpfi = Tpfi;
            solverStruct.Tpfj = Tpfj;
            solverStruct.Tsfi = Tsfi;
            solverStruct.Tsfj = Tsfj;
            solverStruct.Tsfj = Tsfj;
            solverStruct.J = J;
            solverStruct.Gp = GpDiag;
            solverStruct.Gs = Gs;
            solverStruct.G_T = G_T;
            solverStruct.Gell = Gell;
            solverStruct.beta_length = beta_length; 
            solverStruct.solverMatrix = solverMatrix;   
    end
end


function [Binv,Gpi,Gpj,Gsi,Gsj,Tpfi,Tpfj,Tsfi,Tsfj,J,GpDiag,Gs,GsDiag,GsDiagMod] = createKinematicSolverParams(jointPrimStruct, jointSecStruct, numPrimJoints, numSecJoints)
    [~, Binv] = createBMatrix(jointPrimStruct);
    [Gpi, Gpj, Gsi, Gsj] = createDiagConstMat(jointPrimStruct, jointSecStruct);
    [Tpfi, Tpfj] = createGlobJointTransMat(jointPrimStruct);
    [Tsfi, Tsfj] = createGlobJointTransMat(jointSecStruct);
    J = createJMatrix(jointPrimStruct, Binv);
    GpDiag = createGMatrix(jointPrimStruct, numPrimJoints, 'GDiag');
    Gs = createGMatrix(jointSecStruct, numPrimJoints, 'Gs');
    GsDiag = createGMatrix(jointSecStruct, numSecJoints, 'GDiag'); 
    GsDiagMod = createGMatrix(jointSecStruct, numSecJoints, 'GDiagMod'); 
end

function solverStruct = assignSolverStructFieldsDynamic(solverStruct, varargin)
    % ASSIGNSOLVERSTRUCTFIELDSDYNAMIC
    % This function dynamically assigns values to fields in solverStruct.
    %
    % Inputs:
    %   solverStruct - The structure to which fields are to be assigned.
    %   varargin - Pairs of field names and values. Each pair consists of a string
    %              (field name in solverStruct) followed by its corresponding value.
    %
    % Output:
    %   solverStruct - Updated structure with assigned fields.

    % Check that varargin has even number of arguments (pairs of field names and values)
    if mod(length(varargin), 2) ~= 0
        error('Arguments must be provided in pairs of field names and values.');
    end

    for i = 1:2:length(varargin)
        fieldName = varargin{i};
        fieldValue = varargin{i+1};
        solverStruct.(fieldName) = fieldValue;
    end
end
