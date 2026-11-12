function [kMatrixLok, cMatrixLok, cMatrixLokRange, kMatrixLokNullspace] = createLocalCKMatrices(conStruct, conType)
    % ##################################################################################
    % Function: createLocalCKMatrices
    % Description: calculates local stiffnes Matrix 
    %  forceAdd structures based on the provided JSON structure
    %              and the current bodies configuration. This includes setting up
    %              type-specific parameters and calculating initial deflections and load.
    %
    % Inputs:
    %   jsonForceAddStruct - Structure containing force connector's configuration and 
    %                        type information.
    %   bodiesStruct       - Structure containing information about the existing bodies.
    %
    % Output:
    %   forceAddStruct - Initialized structure array for each force connection with
    %                    type-specific details.
    % ##################################################################################
    switch conType 
        
        case 'spring'
            % Create local stiffness matrix (kMatrixLok) for spring pointing in ez direction
            kMatrixLok = diag([zeros(2,1); conStruct.charStruct.k.kValue; zeros(3,1)]);

            % Compute local compliance matrices from stiffness matrices
            cMatrixLok = computeLocCompMat(kMatrixLok);

        case 'bushing'
            % Initialize lokal stiffness matrix (kMatrixLok) based on stiffness values k1-k6
            kMatrixLok = diag([struct2array(conStruct.charStruct.k).kValue]);
                
            % Compute local compliance matrices from stiffness matrices
            cMatrixLok = computeLocCompMat(kMatrixLok);
    end


    if strcmp(conStruct.topology,'primary') || strcmp(conStruct.topology,'secondary')

        if ~isequal(conStruct.j.eBasis, conStruct.i.eBasisJoint)

            % Calculates transformation matrix R from jeBasisBushing to ieBasisJoint                  
            R = conStruct.i.eBasisJoint' * conStruct.j.eBasis;

            % Transforms local k from jeBasisBushing to ieBasisJoint
            kMatrixLokJoint = lokMatToGlobal(kMatrixLok,R);
            cMatrixLokJoint = lokMatToGlobal(cMatrixLok,R);

            % Determine local null space matrix
            [cMatrixLokJointRange, kMatrixLokJointNullspace] = determineNullSpaceContributions(cMatrixLokJoint, kMatrixLokJoint, conStruct.charStruct.NullSpaceMatrix);
                        
            % Transform from ieBasisJoint to jeBasisBushing
            kMatrixLokNullspace = lokMatToGlobal(kMatrixLokJointNullspace, R'); 
            cMatrixLokRange = lokMatToGlobal(cMatrixLokJointRange, R');             

        else
            [cMatrixLokRange, kMatrixLokNullspace] = determineNullSpaceContributions(cMatrixLok, kMatrixLok, conStruct.charStruct.NullSpaceMatrix); 

        end        
          
    else
        % Initialize empty null space matrices for other topologies
        cMatrixLokRange = [];
        kMatrixLokNullspace = [];
    end
end
