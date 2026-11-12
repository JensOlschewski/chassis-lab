function conStruct = fun_createConParam(conStruct)
    % ##################################################################################
    % Function: fun_createConParam
    % Description: Updates the conStruct (which can be either bushings or springs)
    %              by computing transformation matrices, calculating current deflections,
    %              and determining stiffness and damping matrices for each active element.
    %
    % Inputs:
    %   conStruct - Structure array containing connection elements with their
    %               configurations and properties.
    %
    % Outputs:
    %   conStruct - Updated structure array with computed parameters for each
    %               active connection element.
    % ##################################################################################

    % Iterate through each connection element
    for i = 1:length(conStruct)

        % Process only active elements
        if conStruct(i).active  

            % Update velocity transformation matrices
            [conStruct(i).Ti,conStruct(i).Ti_inv] = createJointTransMat(createSkewSymMat(conStruct(i).i.s));
            [conStruct(i).Tj,conStruct(i).Tj_inv] = createJointTransMat(createSkewSymMat(conStruct(i).j.s));

            % Extract charStruct
            charStruct = conStruct(i).charStruct;

            % Handle based on conType
            switch conStruct(i).conType
                
                case 'spring'
                    % Calculate and update deflection
                    conStruct(i).loadStruct.currentDeflection = calculateConMotion(...
                        conStruct(i).i.point, conStruct(i).j.point,...
                        [], [],...
                        'spring',conStruct(i).loadStruct.freeLength);

                    % Determine spring direction
                    deltaPos = conStruct(i).i.point - conStruct(i).j.point;
                    conStruct(i).loadStruct.springDirection = deltaPos ./ norm(deltaPos);

                    % If nonlinear, extract stiffness and load based on currentDeflection
                    if ~charStruct.isLinear 
                        [kValue, currentLoad] = extractStiffness(charStruct, conStruct(i).loadStruct.currentDeflection);
                        charStruct.k.kValue = kValue;
                        conStruct(i).loadStruct.currentLoad = currentLoad + conStruct(i).loadStruct.preload;
                    else
                        % If linear, calculate current load based on kValue and currentDeflection
                        conStruct(i).loadStruct.currentLoad = -charStruct.k.kValue * conStruct(i).loadStruct.currentDeflection + conStruct(i).loadStruct.preload;        
                    end

                    % Calculate stiffness and compliances matrices
                    springDirection = conStruct(i).loadStruct.springDirection;
                    kValue = charStruct.k.kValue;
                    cValue = 1 / kValue;

                    % Tensorprduct from unit vector springDirection
                    tensorSpringDirection = springDirection * springDirection';

                    charStruct.kMatrix = blkdiag((kValue *tensorSpringDirection),zeros(3));
                    charStruct.kMatrixCoup = createMatCoup(charStruct.kMatrix, 'K', conStruct(i).Ti_inv, conStruct(i).Tj_inv);

                    charStruct.cMatrix = blkdiag((cValue *tensorSpringDirection),zeros(3));
                    charStruct.cMatrixCoup = createMatCoup(charStruct.cMatrix, 'C', conStruct(i).Ti, conStruct(i).Tj);

                case 'bushing'
                    % Calculate and update deflection
                    conStruct(i).loadStruct.currentDeflection = calculateConMotion(...
                        conStruct(i).i.point, conStruct(i).j.point,...
                        conStruct(i).i.eBasis, conStruct(i).j.eBasis,...
                        'bushing', conStruct(i).loadStruct.freeLength);
                    
                    % Calculate and update kinematic motion
                    conStruct(i).loadStruct.currentKinMotion = blkdiag(conStruct(i).j.eBasis'*conStruct(i).i.eBasisJoint, conStruct(i).j.eBasis'*conStruct(i).i.eBasisJoint) * calculateConMotion(conStruct(i).i.pointJoint, conStruct(i).j.pointJoint, conStruct(i).i.eBasisJoint, conStruct(i).j.eBasisJoint, 'joint');
                    currentKinMotionJointLocal = calculateConMotion(...
                        conStruct(i).i.pointJoint, conStruct(i).j.pointJoint,...
                        conStruct(i).i.eBasisJoint, conStruct(i).j.eBasisJoint,...
                        'joint');
 
                    % Calculates transformation matrix R from jeBasisBushing to ieBasisJoint                  
                    R = conStruct(i).j.eBasis' * conStruct(i).j.eBasisJoint;   
                    
                    % Transform to bushing coordinate system
                    conStruct(i).loadStruct.currentKinMotion = blkdiag(R,R) * currentKinMotionJointLocal;

                    % Determine current local load
                    conStruct(i).loadStruct.currentLoad = -charStruct.kMatrixLokNullspace * conStruct(i).loadStruct.currentKinMotion + conStruct(i).loadStruct.preload;        


                    if strcmp(conStruct(i).jointType,'Symmetriebedingung')
                        charStruct.cMatrix = (...
                            2 * lokMatToGlobal(charStruct.cMatrixLok, conStruct(i).j.eBasis)...
                            );

                        % kMatrix for each side is needed and tranformed to global 
                        charStruct.kMatrix = lokMatToGlobal(charStruct.kMatrixLokNullspace, conStruct(i).j.eBasis);
                        R = conStruct(i).i.eBasis' * conStruct(i).j.eBasis; % Transformation matrix from jeBasisBushing to ieBasisJoint
                        charStruct.kMatrixISideLokNullspace = lokMatToGlobal(charStruct.kMatrixLokNullspace,R);
                        charStruct.kMatrixISide = lokMatToGlobal(charStruct.kMatrixISideLokNullspace,conStruct(i).i.eBasis);

                        %
                        charStruct.kMatrixCoup = createMatCoup([], 'KiKj', conStruct(i).Ti_inv, conStruct(i).Tj_inv,charStruct.kMatrixISide, charStruct.kMatrix);

                    else
                        % Calculate global matrices
                        charStruct.cMatrix = lokMatToGlobal(charStruct.cMatrixLokRange, conStruct(i).j.eBasis);
                        charStruct.kMatrix = lokMatToGlobal(charStruct.kMatrixLokNullspace, conStruct(i).j.eBasis);

                    end

                    % Determine the correct order for coupling matrices
                    if conStruct(i).isJGreaterThanI
                        charStruct.cMatrixCoup = createMatCoup(charStruct.cMatrix, 'C', conStruct(i).Tj);
                        if ~strcmp(conStruct(i).jointType,'Symmetriebedingung')
                            charStruct.kMatrixCoup = createMatCoup(charStruct.kMatrix, 'K', conStruct(i).Ti_inv, conStruct(i).Tj_inv);
                        end
                    else
                        charStruct.cMatrixCoup = createMatCoup(charStruct.cMatrix, 'C', conStruct(i).Ti);
                        if ~strcmp(conStruct(i).jointType,'Symmetriebedingung')
                            charStruct.kMatrixCoup = createMatCoup(charStruct.kMatrix, 'K', conStruct(i).Tj_inv, conStruct(i).Ti_inv);
                        end
                    end

                otherwise
                    error('Unknown conType: %s', conStruct(i).conType);
            end

            % Assign updated charStruct back to conStruct
            conStruct(i).charStruct = charStruct;
        end
    end
end
