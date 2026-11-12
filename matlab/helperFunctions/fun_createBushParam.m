
function bushStruct = fun_createBushParam(bushStruct)
    % ##################################################################################
    % fun_createBushParam updates bushing parameters based on system configurations
    %
    % This function iterates through each bushing, updating its parameters based on the
    % current system configuration and topology. It specifically updates transformation matrices
    % for velocity and reaction forces, and converts local stiffness and damping matrices to
    % their global representations. For certain bushing topologies, it also updates coupled
    % damping and stiffness matrices.
    %
    % Input:
    %   bushStruct - Array of structs, each representing a bushing in the system.
    %
    % Output:
    %   bushStruct - Updated array of structs with new bushing parameters.
    % ##################################################################################

    % Iterate through each bushing    
    for i = 1:numel(bushStruct)
        % Process only active bushings    
        if bushStruct(i).active  
            % Extract charStruct
            charStruct = bushStruct(i).charStruct; 

            % Update transformation matrices
            bushStruct(i).Tvi = createJointTransMat(createSkewSymMat(bushStruct(i).i.s), 'Velocity');
            bushStruct(i).Tvj = createJointTransMat(createSkewSymMat(bushStruct(i).j.s), 'Velocity');
            bushStruct(i).Tfi = createJointTransMat(createSkewSymMat(bushStruct(i).i.s), 'Reactionforce');
            bushStruct(i).Tfj = createJointTransMat(createSkewSymMat(bushStruct(i).j.s), 'Reactionforce');

            % Calculate and update deflection
            bushStruct(i).loadStruct.currentDeflection = calculateConMotion(...
            bushStruct(i).i.point, bushStruct(i).j.point, ...
            bushStruct(i).i.eBasis, bushStruct(i).j.eBasis,...
            'bushing',bushStruct(i).loadStruct.freeLength);
            
            % Calculate global matrices
            charStruct.cMatrix = createGlobalMatrix(charStruct.cMatrixLokRange, bushStruct(i).j.eBasis);
            charStruct.kMatrix = createGlobalMatrix(charStruct.kMatrixLokNullspace, bushStruct(i).j.eBasis);
            charStruct.cMatrixCoup = createGlobalCoupMatrix(charStruct.cMatrix, 'C', bushStruct(i).isJGreaterThanI, bushStruct(i).Tfi, bushStruct(i).Tfj);
            charStruct.kMatrixCoup = createGlobalCoupMatrix(charStruct.kMatrix, 'K', bushStruct(i).isJGreaterThanI, bushStruct(i).Tfi, bushStruct(i).Tfj);

            % Assign CharStruct Again to BushStruct
            bushStruct(i).charStruct = charStruct;
        end
    end
end

function globalMatrix = createGlobalMatrix(localMatrix, eBasis)
    globalMatrix = lokMatToGlobal(localMatrix, eBasis);
end

function globalMatrixCoup = createGlobalCoupMatrix(globalMatrix, matrixType, isJGreaterThanI, Tfi, Tfj)
    if isJGreaterThanI
        globalMatrixCoup = createMatCoup(globalMatrix, matrixType, Tfi, Tfj);
    else
        globalMatrixCoup = createMatCoup(globalMatrix, matrixType, Tfj, Tfi);
    end
end
