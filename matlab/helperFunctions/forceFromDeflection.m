
function fe = forceFromDeflection(conStruct, numB, varargin)
    % ##################################################################################
    % Function: forceFromDeflection 
    %
    % Description:
    %   Calculates the global force vector at body center markers (CMs) from 
    %   compliant connector elements based on their current deformation   
    %
    %   The function evaluates and aggregates the force contributions from compliant
    %   connector elements constructing a global force
    %   vector representing the combined effect of these elements.
    %
    % Inputs:
    %   conStruct - Array of structures containing force and bushing parameters,
    %                     including stiffness matrices, transformation matrices, deflections,
    %                     and other necessary details.
    %   numB            - Total number of bodies in the system (used for preallocating
    %                     the global force vector).
    %   varargin        - Optional input arguments (e.g., additional parameters or flags).
    %
    % Outputs:
    %   fe          - Global force vector at each center marker for all bodies
    %                     (size: numB*6 x 1).
    % ##################################################################################

    % Initialize global force vector
    fe = zeros(numB * 6, 1);  % Preallocate with zeros for efficiency

    % Process optional input arguments
    if ~isempty(varargin)
        % Handle Relonomic calculation here -> Load stays constant but direction changes
        constantLoad = varargin{1};
        % Check if constantLoad has the same numel as conStruct
        if numel(constantLoad) ~= numel(conStruct)
            error('The number of elements in constantLoad must match the number of elements in conStruct.');
        end
        isReloCase = true;
    else
        isReloCase = false;
    end

    % Iterate over force and bushing elements to compute their force contributions
    for i = 1:numel(conStruct)
        
        % Only consider active elements
        if conStruct(i).active  

            % Determine current type
            currentConType = conStruct(i).conType;

            % Check if 'i and j' body is not ground and extract ColIndex (bodyID ~= 0)
            iColIndex = getColIndex(conStruct(i).i.bodyID);
            jColIndex = getColIndex(conStruct(i).j.bodyID);

            switch currentConType
                case 'spring'

                    if ~isReloCase
                        % Calculate global spring load (6x1 vector)
                        springForce = conStruct(i).loadStruct.currentLoad * conStruct(i).loadStruct.springDirection;
                    else 
                        springForce = constantLoad{i} * conStruct(i).loadStruct.springDirection;
                    end

                    springLoadGlobal = [springForce; zeros(3,1)];  % No torque for a spring

                    % Calculate forces from connector point to center markers -> inv(Tfi), inv(Tfj), consider use of -sVec instead of inv(Tfi) 
                    fCMi = conStruct(i).Ti_inv' * springLoadGlobal;
                    fCMj = conStruct(i).Tj_inv' * -springLoadGlobal;

                    
                    case 'bushing'

                        if ~isReloCase                        
                            % Calculate global bushing load (6x1 vector)
                            bushingLoadLocal = conStruct(i).loadStruct.currentLoad;
                            
                        else
                            % ReloCase transformation of bushingLocal to bushingGlobal
                            bushingLoadLocal = constantLoad{i}; 
                            
                        end
                        % Transform bushingLoadLocal to bushingLoadGlobal
                        bushingLoadGlobal = blkdiag(conStruct(i).j.eBasis,conStruct(i).j.eBasis) * bushingLoadLocal;
                        
                        % Calculate forces from connector point to center markers -> inv(Tfi), inv(Tfj), consider use of -sVec instead of inv(Tfi) 
                        fCMi = conStruct(i).Ti_inv' * bushingLoadGlobal;
                        fCMj = conStruct(i).Tj_inv' * -bushingLoadGlobal; % -> check adams help here maybe add additional torque part?

                        % Add additional cases for other types if needed
                        
                    otherwise
                        error('Unknown Connector Type: %s', currentConType);
            end

            % Update global force vector
            if ~isempty(iColIndex)
                fe(iColIndex) = fe(iColIndex) + fCMi;
            end
            if ~isempty(jColIndex)
                fe(jColIndex) = fe(jColIndex) + fCMj;
            end
        end
    end
end

% #################
% Helper Functions
% #################
    
% ----------------------------------------------------------------
function colIndex = getColIndex(bodyID)
    % getColIndex returns the column index for a given body ID.
    % If the body ID is 0 (ground), it returns an empty array.
    %
    % Inputs:
    %   bodyID - The ID of the body
    %
    % Outputs:
    %   colIndex - The column index for the body, or an empty array if the body is ground

    if bodyID ~= 0
        colIndex = 6 * (bodyID - 1) + (1:6);
    else
        colIndex = [];  % Ground body; no index needed
    end
end
