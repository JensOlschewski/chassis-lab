function forceAddStruct = fun_createForceAddParam(forceAddStruct)
    % ##################################################################################
    % Function: fun_createForceAddParam
    % Description: Updates the forceAddStruct by computing transformation matrices,
    %              calculating current deflections, and determining spring directions
    %              for each active force element.
    %
    % Inputs:
    %   forceAddStruct - Structure array containing force elements with their
    %                    configurations and properties.
    %
    % Outputs:
    %   forceAddStruct - Updated structure array with computed parameters for each
    %                    active force element.
    % ##################################################################################

    % Iterate through each forceAdd element
    for i = 1:length(forceAddStruct)

        % Process only active force elements
        if forceAddStruct(i).active  

            % Update velocity transformation matrices
            forceAddStruct(i).Tvi = createJointTransMat(createSkewSymMat(forceAddStruct(i).i.s), 'Velocity');
            forceAddStruct(i).Tvj = createJointTransMat(createSkewSymMat(forceAddStruct(i).j.s), 'Velocity');

            % Update reaction force transformation matrices
            forceAddStruct(i).Tfi = createJointTransMat(createSkewSymMat(forceAddStruct(i).i.s), 'Reactionforce');
            forceAddStruct(i).Tfj = createJointTransMat(createSkewSymMat(forceAddStruct(i).j.s), 'Reactionforce');

            % Calculate and update deflection
            forceAddStruct(i).loadStruct.currentDeflection = calculateConMotion(...
                forceAddStruct(i).i.point, forceAddStruct(i).j.point,...
                [], [],...
                'spring', forceAddStruct(i).loadStruct.freeLength);

            % Determine spring direction
            deltaPos = forceAddStruct(i).i.point - forceAddStruct(i).j.point;
            forceAddStruct(i).loadStruct.springDirection = deltaPos./norm(deltaPos);

            % Determine currenct charStruct
            charStruct = forceAddStruct(i).charStruct;

            % If nonlinear extract stiffness and load based on currentDeflection
            if ~charStruct.isLinear 
                [kValue,currentLoad] = extractStiffness(charStruct, forceAddStruct(i).loadStruct.currentDeflection);
                charStruct.k.kValue = kValue;
                forceAddStruct(i).loadStruct.currentLoad = currentLoad + forceAddStruct(i).loadStruct.preload;

            % If linear, calculate current load based on kValue and currentDeflection
            elseif charStruct.isLinear 
                forceAddStruct(i).loadStruct.currentLoad = -charStruct.k.kValue * forceAddStruct(i).loadStruct.currentDeflection + forceAddStruct(i).loadStruct.preload;        
            end

            % Calculate global matrices
            % Calculate stiffness and damping matrices
            springDirection = abs(forceAddStruct(i).loadStruct.springDirection);
            kValue = forceAddStruct(i).charStruct.k.kValue;
            cValue = 1 / kValue;

            charStruct.kMatrix = diag([springDirection * kValue; zeros(3,1)]);
            charStruct.kMatrixCoup = createMatCoup(charStruct.kMatrix, 'K', forceAddStruct(i).Tfi, forceAddStruct(i).Tfj);

            charStruct.cMatrix = diag([springDirection * cValue; zeros(3,1)]);
            charStruct.cMatrixCoup = createMatCoup(charStruct.cMatrix, 'C', forceAddStruct(i).Tfi, forceAddStruct(i).Tfj);
            
            % Assign CharStruct Again to forceAddStruct
            forceAddStruct(i).charStruct = charStruct;

        end
    end
end
